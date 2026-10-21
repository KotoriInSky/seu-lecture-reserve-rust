use anyhow::{Context, Result, ensure};
use inquire::{Confirm, MultiSelect, Password, Select, Text};
use seu_lecture_reserve::{
    auth::{SchoolClient, SessionExpired},
    batch::{Backend, BatchOptions, ItemStatus, SubmitOutcome, run_batch},
    config::{self, Config, Credentials, OcrPaths},
    domain::{Lecture, filter_lectures, selected_conflicts},
    ocr::Ocr,
    storage,
};
use std::{path::PathBuf, time::Duration};

fn sms() -> Result<String> {
    Ok(Password::new("学校已发送短信验证码，请输入：")
        .without_confirmation()
        .prompt()?)
}

fn authenticated_read<T>(
    client: &SchoolClient,
    credentials: &Credentials,
    retried: &mut bool,
    read: impl Fn() -> Result<T>,
) -> Result<T> {
    match read() {
        Err(error) if !*retried && error.downcast_ref::<SessionExpired>().is_some() => {
            *retried = true;
            client.clear_session()?;
            client.login(credentials, sms)?;
            read()
        }
        result => result,
    }
}

fn credentials(replace: bool) -> Result<Credentials> {
    let path = config::data_dir()?.join("credentials.dpapi");
    if storage::persistence_available() && !replace {
        match storage::load_secret(&path) {
            Ok(Some(c)) => return Ok(c),
            Ok(None) => {}
            Err(_) => println!("原凭据无法解密，请重新输入。"),
        }
    }
    let card_number = Text::new("一卡通号：").prompt()?.trim().to_owned();
    let password = Password::new("统一认证密码：")
        .without_confirmation()
        .prompt()?;
    ensure!(
        !card_number.is_empty() && !password.is_empty(),
        "账号和密码不能为空"
    );
    let c = Credentials {
        card_number,
        password,
    };
    if storage::persistence_available() {
        storage::save_secret(&path, &c)?;
    }
    Ok(c)
}

fn paths(config: &mut Config) -> Result<()> {
    let old = config.ocr.clone().unwrap_or_default();
    let path = |label: &str, initial: &std::path::Path| -> Result<PathBuf> {
        let initial = initial.to_string_lossy();
        let text = Text::new(label).with_initial_value(&initial).prompt()?;
        let path = PathBuf::from(text.trim().trim_matches('"'));
        ensure!(path.is_file(), "文件不存在：{}", path.display());
        Ok(path)
    };
    config.ocr = Some(OcrPaths {
        model: path("ddddocr common_old.onnx 文件：", &old.model)?,
        dictionary: path("配套 ddddocr 字符配置 JSON 文件：", &old.dictionary)?,
        runtime: path(
            "ONNX Runtime 动态库（Windows .dll / Linux .so）：",
            &old.runtime,
        )?,
    });
    config.save()
}

fn values(prompt: &str, initial: &[String]) -> Result<Vec<String>> {
    let text = Text::new(prompt)
        .with_initial_value(&initial.join("；"))
        .prompt()?;
    Ok(text
        .split(['；', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect())
}

fn preferences(config: &mut Config) -> Result<()> {
    println!(
        "多个值用分号分隔；留空表示不限。指定讲座名称会优先于类别和校区条件，仍检查类型和冲突。"
    );
    let p = &mut config.preferences;
    p.titles = values("指定讲座完整名称：", &p.titles)?;
    p.categories = values("讲座类别：", &p.categories)?;
    p.campuses = values("校区：", &p.campuses)?;
    p.enable_online = Confirm::new("包括线上讲座？")
        .with_default(p.enable_online)
        .prompt()?;
    p.enable_offline = Confirm::new("包括线下讲座？")
        .with_default(p.enable_offline)
        .prompt()?;
    let value = Text::new("场次间缓冲分钟数（0 表示首尾相接允许）：")
        .with_initial_value(&config.buffer_minutes.to_string())
        .prompt()?;
    config.buffer_minutes = value.parse().context("缓冲时间必须是整数")?;
    ensure!(
        (0..=1440).contains(&config.buffer_minutes),
        "缓冲时间应为 0–1440 分钟"
    );
    config.use_system_proxy = Confirm::new("通过系统代理访问学校？通常选择否。")
        .with_default(config.use_system_proxy)
        .prompt()?;
    config.save()
}

struct LiveBackend<'a> {
    client: &'a SchoolClient,
    credentials: &'a Credentials,
    ocr: &'a mut Ocr,
}
impl Backend for LiveBackend<'_> {
    fn bookings(&mut self) -> Result<Vec<Lecture>> {
        self.client.bookings()
    }
    fn reauthenticate(&mut self) -> Result<()> {
        println!("会话已失效，正在重新认证。");
        self.client.clear_session()?;
        self.client.login(self.credentials, sms)
    }
    fn is_auth_error(&self, error: &anyhow::Error) -> bool {
        error.downcast_ref::<SessionExpired>().is_some()
    }
    fn submit(&mut self, lecture: &Lecture) -> SubmitOutcome {
        println!("正在处理：{}", lecture.title);
        if let Err(e) = self.client.precheck(&lecture.id) {
            return if self.is_auth_error(&e) {
                SubmitOutcome::AuthExpired
            } else {
                SubmitOutcome::Failed(e.to_string())
            };
        }
        let image = match self.client.captcha() {
            Ok(image) => image,
            Err(e) => {
                return if self.is_auth_error(&e) {
                    SubmitOutcome::AuthExpired
                } else {
                    SubmitOutcome::Failed("验证码获取失败".into())
                };
            }
        };
        let prediction = match self.ocr.recognize(&image) {
            Ok(s) => s,
            Err(_) => {
                println!("验证码未识别为四位数字，将在次数限制内刷新重试。");
                std::thread::sleep(Duration::from_secs(1));
                return SubmitOutcome::CaptchaRejected;
            }
        };
        let outcome = self.client.reserve(&lecture.id, &prediction);
        if matches!(outcome, SubmitOutcome::CaptchaRejected) {
            std::thread::sleep(Duration::from_secs(1));
        }
        outcome
    }
}

fn run() -> Result<()> {
    println!("东南大学讲座预约 · Rust CLI");
    if !storage::persistence_available() {
        println!("当前平台仅在内存中保留账号和会话，每次启动需重新登录。");
    }
    let mut config = Config::load()?;
    let mode = loop {
        let mut actions = vec![
            "查询并选择讲座预约",
            "只读查询讲座",
            "修改筛选与网络偏好",
            "配置 OCR 文件",
        ];
        if storage::persistence_available() {
            actions.push("更换账号");
        }
        actions.push("退出");
        let action = Select::new("本次操作：", actions).prompt()?;
        match action {
            "修改筛选与网络偏好" => preferences(&mut config)?,
            "配置 OCR 文件" => paths(&mut config)?,
            "更换账号" => {
                credentials(true)?;
                let session = config::data_dir()?.join("session.dpapi");
                if session.exists() {
                    std::fs::remove_file(session)?;
                }
            }
            "退出" => return Ok(()),
            other => break other,
        }
    };
    if mode == "查询并选择讲座预约"
        && Confirm::new("修改上次的筛选偏好？")
            .with_default(false)
            .prompt()?
    {
        preferences(&mut config)?;
    }
    let credentials = credentials(false)?;
    let session = config::data_dir()?.join("session.dpapi");
    let client = match SchoolClient::new(session.clone(), config.use_system_proxy) {
        Ok(client) => client,
        Err(e) if session.exists() => {
            println!("保存的会话不可用，将重新登录：{e}");
            std::fs::rename(&session, session.with_extension("invalid.dpapi"))?;
            SchoolClient::new(session, config.use_system_proxy)?
        }
        Err(e) => return Err(e),
    };
    println!("正在验证登录会话并读取讲座……");
    let mut renewed = false;
    let listing = authenticated_read(&client, &credentials, &mut renewed, || client.lectures())?;
    let booked = authenticated_read(&client, &credentials, &mut renewed, || client.bookings())?;
    client.save_session()?;
    println!(
        "当前 {} 场讲座，{} 条已有预约记录。",
        listing.lectures.len(),
        booked.len()
    );
    for (i, l) in listing.lectures.iter().enumerate() {
        println!(
            "{}. {}\n   {} | {}–{} | {}\n   {}",
            i + 1,
            l.title,
            l.category,
            l.start.format("%m-%d %H:%M"),
            l.end.format("%H:%M"),
            l.location,
            listing.statuses[&l.id]
        );
    }
    if mode == "只读查询讲座" {
        return Ok(());
    }
    let candidates = filter_lectures(
        &listing.lectures,
        &config.preferences,
        &booked,
        config.buffer_minutes,
    )?;
    if candidates.is_empty() {
        println!("当前没有符合筛选条件且可预约的讲座，本次运行结束。");
        return Ok(());
    }
    let selected = loop {
        let labels: Vec<_> = candidates
            .iter()
            .enumerate()
            .map(|(i, l)| {
                format!(
                    "{} · {} · {}",
                    i + 1,
                    l.title,
                    l.start.format("%m-%d %H:%M")
                )
            })
            .collect();
        let defaults: Vec<_> = candidates
            .iter()
            .enumerate()
            .filter(|(_, l)| config.last_selected_ids.contains(&l.id))
            .map(|(i, _)| i)
            .collect();
        let choices =
            MultiSelect::new("选择本次预约的讲座（空格选择，回车继续）：", labels.clone())
                .with_default(&defaults)
                .prompt()?;
        let selected: Vec<_> = choices
            .iter()
            .filter_map(|s| labels.iter().position(|v| v == s))
            .map(|i| candidates[i].clone())
            .collect();
        if selected.is_empty() {
            println!("未选择讲座，本次运行结束。");
            return Ok(());
        }
        config.last_selected_ids = selected.iter().map(|l| l.id.clone()).collect();
        let conflicts = selected_conflicts(&selected, config.buffer_minutes)?;
        if conflicts.is_empty() {
            break selected;
        }
        println!("以下所选讲座时间冲突，请重新选择：");
        for (a, b) in conflicts {
            let title = |id: &str| {
                selected
                    .iter()
                    .find(|l| l.id == id)
                    .map(|l| l.title.as_str())
                    .unwrap_or("")
            };
            println!("  {} ↔ {}", title(&a), title(&b));
        }
    };
    config.save()?;
    if config.ocr.is_none() {
        println!("预约需要配置本地 OCR 模型。");
        paths(&mut config)?;
    }
    let mut ocr = Ocr::load(config.ocr.as_ref().unwrap())?;
    if !Confirm::new(&format!("开始预约选中的 {} 场讲座？", selected.len()))
        .with_default(false)
        .prompt()?
    {
        return Ok(());
    }
    let mut backend = LiveBackend {
        client: &client,
        credentials: &credentials,
        ocr: &mut ocr,
    };
    let reports = run_batch(
        &mut backend,
        &selected,
        BatchOptions {
            buffer_minutes: config.buffer_minutes,
            captcha_retries: 2,
        },
    )?;
    println!("\n本次预约结果：");
    for report in reports {
        let status = match report.status {
            ItemStatus::Success => "成功",
            ItemStatus::Failed => "失败",
            ItemStatus::Skipped => "跳过",
            ItemStatus::Uncertain => "结果待核实",
        };
        let title = selected
            .iter()
            .find(|l| l.id == report.id)
            .map(|l| l.title.as_str())
            .unwrap_or(&report.id);
        println!("[{status}] {title} — {}", report.reason);
    }
    client.save_session()?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        if error
            .downcast_ref::<inquire::InquireError>()
            .is_some_and(|e| {
                matches!(
                    e,
                    inquire::InquireError::OperationCanceled
                        | inquire::InquireError::OperationInterrupted
                )
            })
        {
            println!("已取消。");
        } else {
            eprintln!("操作未完成：{error:#}");
            std::process::exit(1);
        }
    }
}
