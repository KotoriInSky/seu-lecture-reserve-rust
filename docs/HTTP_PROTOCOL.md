# 已核实的学校协议

协议核实日期：2026-10-15；Windows 成功预约核验更新于 2026-10-21。本文不记录账号、Cookie、认证票据或个人历史预约明细。

## 接入与认证

入口为 `https://ehall.seu.edu.cn/gsapp/sys/yddjzxxtjappseu/*default/index.do`。已在 Windows 直连环境验证访问；其他网络需要分别检查。开发环境默认代理曾将请求转至 VPN 门户，默认 Python 标识曾返回 403。

入口 302 的 Location 指向 `auth.seu.edu.cn/dist/`，service 位于 fragment；需在重定向丢弃 fragment 前解析。官网认证前端证据：`https://auth.seu.edu.cn/dist/umi.32c0b43a.js`。

认证前缀 `https://auth.seu.edu.cn/auth/casback/`：

| 接口 | 方法 | 行为 |
| --- | --- | --- |
| getChiperKey | POST JSON `{}` | 返回 URL-safe Base64 DER RSA 公钥 |
| casLogin | POST JSON | username、RSA PKCS#1 v1.5 加密密码、service、loginType=account 等；200 成功，502 要求短信 |
| sendStage2Code | POST JSON userId | 官网使用的短信发送接口，本次未触发短信实测 |
| loginRedirect | GET redirectUrl | 响应中的 redirectUrl 已 percent-encode，不能再次编码 |

短信由用户在终端输入，经 RSA 加密为 mobileVerifyCode。登录图形验证码分支未实现，遇到相应挑战时明确停止。登录有效性最终由业务查询核验。

## 业务接口

前缀 `https://ehall.seu.edu.cn/gsapp/sys/yddjzxxtjappseu/modules/hdyy/`。

官网证据：`modules/hdyy/hdyy.js`、`public/components/activity/activity.js`。POST 使用表单参数。

| 接口 | 方法 / 参数 | 结果 |
| --- | --- | --- |
| queryActivityList.do | POST | code=0，datas 包含 hdlbList、hdrsList、xsxxList、xtdqsj |
| queryMyActivityList.do | POST param={} | code=0，datas.wdhdList；缺少列表视为错误 |
| appiontCheck.do | GET wid | 官网提交前检查，要求 code=0 |
| vcode.do | GET _ 时间戳 | code=0，datas 为 JPEG base64 data URL；已获取验证 |
| addReservation.do | POST wid、vcode | 官网判断解包后的 datas=1；本工具另查预约记录核验，Windows 已于 2026-10-21 实测成功 |

讲座 ID 为 WID，已有预约优先使用 HD_WID。标题 JZMC、类别 JZXL_DISPLAY、地点 JZDD、开始/结束 JZSJ/HDJSSJ。时间按 UTC+08:00 解析，以响应中的服务端时间判断开放状态。

预约开放和截止时间分别为 `YYKSSJ`、`YYJSSJ`；总名额为 `HDZRS`，已预约人数来自 `hdrsList[].YYRS`。

验证码错误响应已观察到 `code=-1`，提示“验证码错误，请重试！注意不要同时使用多台设备进行预约操作。”独立登录会话之间也观察到验证码相互失效，因此客户端按顺序完成取验证码、识别和提交。

## 待实测

- 短信二次认证实际收发。
- Linux 的真实预约提交及记录核验；Windows 已于 2026-10-21 完成。
- 学校后续字段及认证机制变化。
