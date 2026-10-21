# Linux 与 WSL2

已在 WSL2 Ubuntu 24.04 x86_64 上编译和运行原生 Linux 程序。Linux 每次启动都需要输入账号和密码；程序只把它们留在内存，不保存明文文件，也不读取 Windows 的 DPAPI 文件。

## 编译

Ubuntu 先安装构建工具，再通过 [rustup](https://rustup.rs/) 安装 Rust stable：

```bash
sudo apt update
sudo apt install build-essential ca-certificates curl
```

安装好 Rust 后：

```bash
source "$HOME/.cargo/env"
git clone https://github.com/KotoriInSky/seu-lecture-reserve-rust.git
cd seu-lecture-reserve-rust
cargo build --release --locked
./target/release/seu-lecture-reserve
```

先运行 `只读查询讲座` 检查网络和登录。这一步不需要模型或 ONNX Runtime。

如果源码放在 WSL 的 `/mnt/c` 或 `/mnt/e` 下，建议把编译产物放进 Linux 文件系统：

```bash
export CARGO_TARGET_DIR="$HOME/.cache/seu-lecture-reserve/target"
cargo build --release --locked
"$CARGO_TARGET_DIR/release/seu-lecture-reserve"
```

Windows 和 Linux 使用不同的 target 目录，避免混用编译产物。

## OCR

`common_old.onnx` 和 `ddddocr-charset.json` 可以与 Windows 共用。Linux 需要 `libonnxruntime.so` 或带版本号的 `.so` 文件，不能使用 Windows DLL。准备步骤见 [OCR 配置](OCR_MODEL.md)。

在 `配置 OCR 文件` 中填写三个文件的 Linux 绝对路径。WSL 访问 Windows 磁盘时，路径写成 `/mnt/c/...` 或 `/mnt/e/...`。同目录保留运行库附带的 `libonnxruntime_providers_shared.so`。

普通偏好默认保存在 `~/.local/share/lecturereserve/preferences.toml`；设置了 `XDG_DATA_HOME` 时则位于 `$XDG_DATA_HOME/lecturereserve/preferences.toml`。Windows 和 Linux 各自保存配置。

## 网络

程序默认直连学校。需要代理时，在 `修改筛选与网络偏好` 中开启系统代理，再为启动程序的终端设置正确的代理环境变量。

WSL NAT 中的 `127.0.0.1` 指向 Linux 自己，通常不能直接访问只监听 Windows localhost 的代理。代理地址是否可用，需要结合 WSL 的网络模式和代理监听地址检查。

本次 WSL 测试中，直连学校超时，现有代理又把入口转到了 VPN 门户。最终通过临时 Windows 转发完成了登录和查询；这个转发工具不包含在项目里。因此，编译成功并不代表当前网络已经能访问学校。

## 已测到哪一步

2026-10-21 的 WSL 测试覆盖了原生构建、终端菜单、CPU OCR、真实登录和只读查询。本地 32 项自动化测试通过。Linux 真实预约、短信二次认证、其他发行版和 ARM64 尚未实测。
