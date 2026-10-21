# 开发说明

项目使用 Rust edition 2024，通过 `Cargo.lock` 固定依赖。需要 Rust stable、rustfmt 和 clippy。Windows 使用 MSVC 工具链及 Visual Studio C++ Build Tools；Ubuntu 安装 `build-essential`。

## 检查和构建

```sh
cargo fmt -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

自动化测试使用模拟响应，不连接学校，也不会产生预约。GitHub Actions 在 Windows 和 Ubuntu 上运行这些检查。

输出位于 `target/release/`。Windows 文件名为 `seu-lecture-reserve.exe`，Linux 为 `seu-lecture-reserve`。项目以源码形式发布，用户自行编译。

## 真实接口检查

```sh
cargo run --locked --example check_connection
```

该示例只登录和查询。Windows 默认读取主程序保存的加密凭据；Linux 在终端询问账号和密码。

示例另有 `--credentials-stdin` 参数，从标准输入接收含 `card_number` 和 `password` 的 JSON，并强制使用内存会话。可由可信进程通过管道传递凭据，避免把密码写进命令行或临时文件。主程序仍通过交互菜单使用。

## 项目结构

| 文件 | 内容 |
| --- | --- |
| `src/main.rs` | 菜单、账号输入和批次入口 |
| `src/auth.rs` | 统一认证和 Cookie 会话 |
| `src/api.rs` | 业务请求及响应解析 |
| `src/domain.rs` | 筛选和时间冲突 |
| `src/batch.rs` | 顺序预约、重试和结果核验 |
| `src/ocr.rs` | 图片预处理、ONNX 推理和 CTC 解码 |
| `src/config.rs`、`src/storage.rs` | 配置及加密存储 |
| `tests/core.rs` | 模拟后端业务测试 |

模型与运行库的准备方法见 [OCR 配置](OCR_MODEL.md)。
