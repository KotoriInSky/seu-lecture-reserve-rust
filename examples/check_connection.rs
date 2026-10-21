//! Read-only live check. --credentials-stdin reads JSON without writing secrets.
use anyhow::{Context, Result, ensure};
use seu_lecture_reserve::{
    auth::SchoolClient,
    config::{self, Config, Credentials},
    storage,
};

fn main() -> Result<()> {
    let dir = config::data_dir()?;
    let config = Config::load()?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.is_empty() || args == ["--credentials-stdin"],
        "用法：check_connection [--credentials-stdin]"
    );
    let (credentials, client) = if !args.is_empty() {
        let credentials: Credentials = serde_json::from_reader(std::io::stdin().lock())
            .context("标准输入应为包含 card_number 和 password 的 JSON")?;
        (
            credentials,
            SchoolClient::new_ephemeral(config.use_system_proxy)?,
        )
    } else if storage::persistence_available() {
        (
            storage::load_secret(&dir.join("credentials.dpapi"))?
                .context("请先运行主程序配置账号")?,
            SchoolClient::new(dir.join("session.dpapi"), config.use_system_proxy)?,
        )
    } else {
        (
            Credentials {
                card_number: inquire::Text::new("一卡通号：").prompt()?,
                password: inquire::Password::new("统一认证密码：")
                    .without_confirmation()
                    .prompt()?,
            },
            SchoolClient::new_ephemeral(config.use_system_proxy)?,
        )
    };
    ensure!(
        !credentials.card_number.trim().is_empty() && !credentials.password.is_empty(),
        "账号和密码不能为空"
    );
    client.login(&credentials, || {
        Ok(inquire::Text::new("请输入学校发送的短信验证码：").prompt()?)
    })?;
    let list = client.lectures()?;
    println!("当前讲座：{} 场", list.lectures.len());
    for lecture in &list.lectures {
        println!("{} | {}", lecture.title, list.statuses[&lecture.id]);
    }
    println!("已有预约记录：{} 条", client.bookings()?.len());
    client.save_session()?;
    println!("纯 Rust 登录与只读查询成功；没有提交预约。");
    Ok(())
}
