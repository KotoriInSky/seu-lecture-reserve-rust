# 东南大学讲座预约 · Rust CLI

东南大学研究生讲座预约工具，支持筛选、多选、验证码识别和时间冲突检查。

版本 `1.0.0`，自行编译使用。

## 编译

安装 [Rust stable](https://rustup.rs/)。Windows 另需 Visual Studio C++ Build Tools；Ubuntu 需要 `build-essential` 和 `ca-certificates`。

```sh
git clone https://github.com/KotoriInSky/seu-lecture-reserve-rust.git
cd seu-lecture-reserve-rust
cargo build --release --locked
```

Windows 运行 `.\target\release\seu-lecture-reserve.exe`；Linux 运行 `./target/release/seu-lecture-reserve`。

## 使用

1. 选择 `只读查询讲座`，输入一卡通号和统一认证密码。
2. 按 [OCR 配置](docs/OCR_MODEL.md) 准备模型、字符表和运行库，在 `配置 OCR 文件` 中填写路径。
3. 设置筛选条件，选择 `查询并选择讲座预约`。空格勾选、回车继续，确认后逐场提交。

程序核验预约结果后退出。当前不提供定时、候补或并发预约。OCR 文件需另行准备，程序运行无需 Python、浏览器或显卡。

操作和筛选规则见 [使用指南](docs/USER_GUIDE.md)。

## 平台

| 平台 | 验证范围 |
| --- | --- |
| Windows x64 | 登录、OCR、预约及结果核验 |
| Linux x64（WSL2 Ubuntu） | 构建、OCR、登录和查询；测试使用 Windows 转发，直连及真实预约待验证 |

macOS 暂未测试。详见 [Linux 与 WSL2](docs/LINUX.md)、[开发说明](docs/DEVELOPMENT.md)及[学校接口](docs/HTTP_PROTOCOL.md)。

## 许可与来源

源码采用 [GPL-3.0-only](LICENSE)。第三方依赖和模型适用各自的许可。

基于 [SEU_Lecture_Reserve](https://github.com/ApricityWang/SEU_Lecture_Reserve) 改写，OCR 使用 [ddddocr](https://github.com/sml2h3/ddddocr) 和 [ONNX Runtime](https://github.com/microsoft/onnxruntime)。
