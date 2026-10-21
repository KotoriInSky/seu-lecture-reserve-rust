# 东南大学讲座预约 · Rust CLI

在终端里查询东南大学研究生讲座，选好场次后确认预约。程序会识别预约验证码、检查时间冲突，并通过预约记录确认结果。

当前版本为 `1.0.0`，通过源码编译使用。每次处理一批手动选择的场次，按顺序提交，完成后退出。没有定时开抢、后台候补或并发预约功能。

## 开始使用

先安装 [Rust stable](https://rustup.rs/)。Windows 需要 Visual Studio C++ Build Tools；Ubuntu 需要 `build-essential` 和 `ca-certificates`。

```sh
git clone https://github.com/KotoriInSky/seu-lecture-reserve-rust.git
cd seu-lecture-reserve-rust
cargo build --release --locked
```

Windows 在 PowerShell 中运行：

```powershell
.\target\release\seu-lecture-reserve.exe
```

Linux 运行：

```bash
./target/release/seu-lecture-reserve
```

首次使用建议先选 `只读查询讲座`，输入一卡通号和统一认证密码，确认能登录并读到列表。这一步不需要 OCR 文件，也不会提交预约。查询结束后程序会退出，再次运行即可回到菜单。

要预约时，先按 [OCR 配置](docs/OCR_MODEL.md) 准备模型、字符表和对应平台的 ONNX Runtime，在菜单的 `配置 OCR 文件` 中填写路径。运行程序本身不需要 Python、浏览器或显卡。

随后选择 `查询并选择讲座预约`：方向键移动，空格勾选，回车继续。确认选中的场次后，程序才会提交。完整步骤和筛选例子见 [使用指南](docs/USER_GUIDE.md)。

## 使用时容易弄错的地方

- 讲座名称、类别和校区按完整文字匹配，填关键词不会命中。
- 填了指定名称后，程序忽略类别和校区条件，但仍检查线上/线下类型和时间冲突。
- 已满、未开放或已结束预约的场次不能选择。当前没有候补等待功能。
- 刚安装时线上、线下都开启；只想参加线上讲座，需要在偏好里关闭线下。
- 结果不明确时，先到学校系统查看预约记录，再决定是否重试。

## 平台和数据

Windows x64 已实测登录、OCR、预约和结果核验。Linux x64 已在 WSL2 Ubuntu 24.04 验证构建、菜单、OCR、登录和只读查询；该环境连接学校时使用了临时 Windows 转发，Linux 直连和真实预约仍待验证。macOS 暂未测试。网络配置见 [Linux 与 WSL2](docs/LINUX.md)。

Windows 用当前用户的 DPAPI 加密保存账号和会话。Linux 每次启动重新输入账号，凭据和 Cookie 只保留在内存。普通偏好保存在本地 TOML 文件中，具体位置见使用指南。

本仓库提供源码，不包含 OCR 模型、运行库、账号数据或验证码采集样本。模型准备过程会用到 Python；文件准备好后即可独立运行 Rust 程序。

## 文档

- [使用指南](docs/USER_GUIDE.md)：首次配置、菜单、筛选和常见问题
- [OCR 配置](docs/OCR_MODEL.md)：模型、字符表和运行库
- [Linux 与 WSL2](docs/LINUX.md)：编译、路径和网络说明
- [开发说明](docs/DEVELOPMENT.md)：项目结构和测试
- [学校接口](docs/HTTP_PROTOCOL.md)：认证、查询和预约协议

## 许可与来源

本项目源码采用 [GNU GPL v3.0](LICENSE)，SPDX 标识为 `GPL-3.0-only`。模型、ONNX Runtime 和 Rust 依赖分别适用各自的许可。

项目起于 [SEU_Lecture_Reserve](https://github.com/ApricityWang/SEU_Lecture_Reserve) 的使用与改写。本仓库收录 Rust 客户端，OCR 使用 [ddddocr](https://github.com/sml2h3/ddddocr) 模型和 [ONNX Runtime](https://github.com/microsoft/onnxruntime)。
