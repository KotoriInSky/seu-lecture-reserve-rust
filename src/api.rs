use crate::auth::{SchoolClient, SessionExpired, checked_response, send};
use crate::batch::SubmitOutcome;
use crate::domain::Lecture;
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, FixedOffset, NaiveDateTime, TimeZone};
use serde_json::Value;
use std::collections::BTreeMap;

pub struct Listing {
    pub lectures: Vec<Lecture>,
    pub statuses: BTreeMap<String, String>,
}
fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}
fn field(value: &Value, key: &str) -> Result<String> {
    scalar(&value[key])
        .filter(|s| !s.trim().is_empty())
        .with_context(|| format!("学校数据缺少字段 {key}"))
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    value[key]
        .as_array()
        .with_context(|| format!("学校数据缺少列表 {key}"))
}
fn data(value: &Value) -> Result<&Value> {
    if scalar(&value["code"]).as_deref() == Some("401") {
        return Err(SessionExpired.into());
    }
    ensure!(
        scalar(&value["code"]).as_deref() == Some("0"),
        "学校接口业务错误：{}",
        value["msg"].as_str().unwrap_or("未知错误")
    );
    value
        .get("datas")
        .filter(|v| !v.is_null())
        .context("学校响应缺少 datas")
}
fn date(value: &str) -> Result<DateTime<FixedOffset>> {
    let local = NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .context("学校时间字段格式不正确")?;
    FixedOffset::east_opt(8 * 3600)
        .unwrap()
        .from_local_datetime(&local)
        .single()
        .context("学校时间无效")
}
fn lecture(row: &Value, reservable: bool) -> Result<Lecture> {
    let location = field(row, "JZDD")?;
    let campus = scalar(&row["SZXQ_DISPLAY"]).unwrap_or_else(|| {
        location
            .find("校区")
            .map(|p| location[..p + "校区".len()].to_owned())
            .unwrap_or_else(|| location.clone())
    });
    let title = field(row, "JZMC")?;
    let result = Lecture {
        id: match scalar(&row["HD_WID"]).filter(|s| !s.is_empty()) {
            Some(id) => id,
            None => field(row, "WID")?,
        },
        online: title.contains("线上")
            || location.contains("腾讯会议")
            || location.contains("线上"),
        title,
        category: field(row, "JZXL_DISPLAY")?,
        campus,
        location,
        start: date(&field(row, "JZSJ")?)?,
        end: date(&field(row, "HDJSSJ")?)?,
        reservable,
    };
    result.validate()?;
    Ok(result)
}
pub fn parse_listing(value: &Value) -> Result<Listing> {
    let d = data(value)?;
    let now = date(&field(d, "xtdqsj")?)?;
    let rows = array(d, "hdlbList")?;
    let seats = array(d, "hdrsList")?;
    let booked = array(d, "xsxxList")?;
    let mut listing = Listing {
        lectures: Vec::new(),
        statuses: BTreeMap::new(),
    };
    for row in rows {
        let id = field(row, "WID")?;
        if listing.statuses.contains_key(&id) {
            continue;
        }
        let total = field(row, "HDZRS")?
            .parse::<u64>()
            .context("讲座名额无效")?;
        let count = seats
            .iter()
            .find(|v| scalar(&v["HD_WID"]).as_deref() == Some(&id))
            .map(|v| field(v, "YYRS")?.parse::<u64>().context("已预约人数无效"))
            .transpose()?
            .unwrap_or(0);
        let has_booking = booked.iter().any(|v| {
            scalar(&v["WID"]).as_deref() == Some(&id)
                && scalar(&v["YY_WID"]).is_some_and(|s| !s.is_empty())
        });
        let status = if field(row, "FBZT")? == "-1" {
            "已取消"
        } else if has_booking {
            "已预约"
        } else if now < date(&field(row, "YYKSSJ")?)? {
            "未开放"
        } else if now > date(&field(row, "YYJSSJ")?)? {
            "预约已结束"
        } else if count >= total {
            "名额已满"
        } else {
            "可预约"
        };
        listing.lectures.push(lecture(row, status == "可预约")?);
        listing
            .statuses
            .insert(id, format!("{status}（{count}/{total}）"));
    }
    Ok(listing)
}
pub fn parse_bookings(value: &Value) -> Result<Vec<Lecture>> {
    let d = data(value)?;
    let mut result = Vec::new();
    for row in array(d, "wdhdList")? {
        if scalar(&row["FBZT"]).as_deref() == Some("-1") {
            continue;
        }
        let l = lecture(row, false)?;
        if !result.iter().any(|b: &Lecture| b.id == l.id) {
            result.push(l);
        }
    }
    Ok(result)
}
pub fn parse_submit(value: &Value) -> SubmitOutcome {
    let code = scalar(&value["code"]);
    if code.as_deref() == Some("401") {
        return SubmitOutcome::AuthExpired;
    }
    if code.as_deref() == Some("0") && scalar(&value["datas"]).as_deref() == Some("1") {
        return SubmitOutcome::Success;
    }
    if code.as_deref().is_some_and(|c| c != "0") {
        let message = value["msg"]
            .as_str()
            .unwrap_or("预约被学校服务器拒绝")
            .to_owned();
        if message.contains("验证码")
            && ["错误", "不正确", "失效", "过期"]
                .iter()
                .any(|t| message.contains(t))
        {
            return SubmitOutcome::CaptchaRejected;
        }
        return SubmitOutcome::Failed(message);
    }
    SubmitOutcome::Uncertain("预约结果格式未知，需要查询预约记录确认".into())
}

fn parse_precheck(result: &Value) -> Result<()> {
    if scalar(&result["code"]).as_deref() == Some("401") {
        return Err(SessionExpired.into());
    }
    ensure!(
        scalar(&result["code"]).as_deref() == Some("0"),
        "{}",
        result["msg"].as_str().unwrap_or("学校预约检查未通过")
    );
    Ok(())
}

const API: &str = "https://ehall.seu.edu.cn/gsapp/sys/yddjzxxtjappseu/modules/hdyy/";
impl SchoolClient {
    fn business_json(&self, request: reqwest::blocking::RequestBuilder) -> Result<Value> {
        let response =
            checked_response(send(request.header("X-Requested-With", "XMLHttpRequest"))?)?;
        response
            .json()
            .map_err(|e| anyhow::anyhow!(e.without_url()))
            .context("学校接口返回了无法识别的数据")
    }
    pub fn lectures(&self) -> Result<Listing> {
        parse_listing(
            &self.business_json(
                self.http
                    .post(format!("{API}queryActivityList.do"))
                    .form(&[("param", "{}")]),
            )?,
        )
    }
    pub fn bookings(&self) -> Result<Vec<Lecture>> {
        parse_bookings(
            &self.business_json(
                self.http
                    .post(format!("{API}queryMyActivityList.do"))
                    .form(&[("param", "{}")]),
            )?,
        )
    }
    pub fn precheck(&self, id: &str) -> Result<()> {
        let result = self.business_json(
            self.http
                .get(format!("{API}appiontCheck.do"))
                .query(&[("wid", id)]),
        )?;
        parse_precheck(&result)
    }
    pub fn captcha(&self) -> Result<Vec<u8>> {
        let response = self.business_json(
            self.http
                .get(format!("{API}vcode.do"))
                .query(&[("_", chrono::Utc::now().timestamp_millis().to_string())]),
        )?;
        let encoded = data(&response)?.as_str().context("验证码格式无效")?;
        let (prefix, content) = encoded.split_once(',').context("验证码缺少图像数据")?;
        ensure!(
            matches!(prefix, "data:image/jpeg;base64" | "data:image/png;base64")
                && content.len() < 2_000_000,
            "验证码图片类型或大小不符合预期"
        );
        STANDARD.decode(content).context("验证码图像解码失败")
    }
    pub fn reserve(&self, id: &str, captcha: &str) -> SubmitOutcome {
        if captcha.len() != 4 || !captcha.bytes().all(|c| c.is_ascii_digit()) {
            return SubmitOutcome::CaptchaRejected;
        }
        match self.business_json(
            self.http
                .post(format!("{API}addReservation.do"))
                .form(&[("wid", id), ("vcode", captcha)]),
        ) {
            Ok(v) => parse_submit(&v),
            Err(e) if e.downcast_ref::<SessionExpired>().is_some() => SubmitOutcome::AuthExpired,
            Err(_) => SubmitOutcome::Uncertain("预约请求异常，先核对预约记录".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture() -> Value {
        json!({"code":0,"datas":{
        "xtdqsj":"2026-10-15 11:00:00","hdlbList":[{
            "WID":"sample","JZMC":"【线下】示例讲座","JZXL_DISPLAY":"其他","JZDD":"九龙湖校区报告厅","FBZT":"1",
            "JZSJ":"2026-10-15 13:30:00","HDJSSJ":"2026-10-15 15:30:00",
            "YYKSSJ":"2026-10-14 19:00:00","YYJSSJ":"2026-10-15 12:00:00","HDZRS":"650"}],
        "hdrsList":[{"HD_WID":"sample","YYRS":650}],"xsxxList":[]}})
    }
    #[test]
    fn full_capacity_is_not_reservable() {
        let listing = parse_listing(&fixture()).unwrap();
        assert!(!listing.lectures[0].reservable);
        assert_eq!(listing.lectures[0].campus, "九龙湖校区");
        assert!(listing.statuses["sample"].contains("满"));
    }
    #[test]
    fn open_seat_and_booking_state_are_distinct() {
        let mut data = fixture();
        data["datas"]["hdrsList"][0]["YYRS"] = json!(649);
        assert!(parse_listing(&data).unwrap().lectures[0].reservable);
        data["datas"]["xsxxList"] = json!([{"WID":"sample","YY_WID":"reservation"}]);
        assert!(!parse_listing(&data).unwrap().lectures[0].reservable);
    }
    #[test]
    fn missing_booking_data_is_not_empty_bookings() {
        assert!(parse_bookings(&json!({"code":0,"datas":{}})).is_err());
        assert!(parse_bookings(&json!({"code":401,"datas":{"wdhdList":[]}})).is_err());
        assert!(
            parse_bookings(&json!({"code":0,"datas":{"wdhdList":[]}}))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn success_requires_exact_envelope_and_result() {
        assert!(matches!(
            parse_submit(&json!({"code":0,"datas":1})),
            SubmitOutcome::Success
        ));
        assert!(matches!(
            parse_submit(&json!({"code":0,"datas":"页面成功提示"})),
            SubmitOutcome::Uncertain(_)
        ));
        assert!(matches!(
            parse_submit(&json!({"code":500,"msg":"失败"})),
            SubmitOutcome::Failed(_)
        ));
    }
    #[test]
    fn precheck_auth_expiry_is_recoverable() {
        let error = parse_precheck(&json!({"code":401,"msg":"expired"})).unwrap_err();
        assert!(error.downcast_ref::<SessionExpired>().is_some());
    }
    #[test]
    fn booking_can_use_hd_wid_without_wid() {
        let mut row = fixture()["datas"]["hdlbList"][0].clone();
        row.as_object_mut().unwrap().remove("WID");
        row["HD_WID"] = json!("existing");
        let value = json!({"code":0,"datas":{"wdhdList":[row]}});
        assert_eq!(parse_bookings(&value).unwrap()[0].id, "existing");
    }
}
