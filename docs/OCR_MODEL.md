# ddddocr 模型配置

以下文件准备示例使用 Windows。Linux/WSL2 可共用模型和字符表，但需要 Linux ONNX Runtime `.so` 动态库，见 [Linux 与 WSL2](LINUX.md)。

## 运行所需文件

| 文件 | 用途 |
| --- | --- |
| common_old.onnx | ddddocr 1.6.1 默认模型，约 13.6 MB |
| ddddocr-charset.json | 完整字符表及预处理配置 |
| onnxruntime.dll | Windows x64 CPU 推理运行库 |
| onnxruntime_providers_shared.dll | 与主 DLL 配套，保留在同目录 |

在“配置 OCR 文件”中填写前三项路径。模型可放任意目录，推荐 `%LOCALAPPDATA%\SEU\LectureReserve\models`。移动文件后需重新配置。

只读查询无需 OCR。正式 EXE 直接执行 ONNX，不需要安装 Python。

## 准备模型（Windows）

以下命令在仓库根目录执行，需要 Python 3.11。创建隔离环境，固定已验证的包版本：

```powershell
python -m venv .local/ocr/venv
.\.local\ocr\venv\Scripts\python.exe -m pip install ddddocr==1.6.1 onnxruntime==1.31.0
```

导出默认模型及完整字符配置：

```powershell
@'
import json
import shutil
from pathlib import Path
import ddddocr

output = Path('.local/ocr/ddddocr')
output.mkdir(parents=True, exist_ok=True)
ocr = ddddocr.DdddOcr(show_ad=False)
charset = ocr.ocr_engine.get_charset()
assert len(charset) == 8210 and charset[0] == ''
config = dict(charset=charset, word=False, image=[-1, 64], channel=1)
text = json.dumps(config, ensure_ascii=False)
(output / 'ddddocr-charset.json').write_text(text, encoding='utf-8')
source = Path(ddddocr.__file__).parent / 'common_old.onnx'
shutil.copyfile(source, output / 'common_old.onnx')
print(output.resolve())
'@ | .\.local\ocr\venv\Scripts\python.exe
```

生成的 `common_old.onnx` 与 `ddddocr-charset.json` 可直接交给 CLI 使用。

DLL 位于 `.local/ocr/venv/Lib/site-packages/onnxruntime/capi`。将其中 `onnxruntime.dll` 的完整路径填入菜单，并保留同目录的配套 DLL。模型和运行库分别适用原项目的许可。

## 准备模型（Linux）

需要 Python 3.11 或更高版本及 venv。Ubuntu 缺少 venv 时先安装 `python3-venv`。在仓库根目录执行：

```bash
python3 -m venv .local/ocr/venv
.local/ocr/venv/bin/python -m pip install ddddocr==1.6.1 onnxruntime==1.31.0
.local/ocr/venv/bin/python - <<'PY'
import json
import shutil
from pathlib import Path
import ddddocr
import onnxruntime

output = Path('.local/ocr/ddddocr')
output.mkdir(parents=True, exist_ok=True)
ocr = ddddocr.DdddOcr(show_ad=False)
charset = ocr.ocr_engine.get_charset()
assert len(charset) == 8210 and charset[0] == ''
config = dict(charset=charset, word=False, image=[-1, 64], channel=1)
(output / 'ddddocr-charset.json').write_text(
    json.dumps(config, ensure_ascii=False), encoding='utf-8')
shutil.copyfile(Path(ddddocr.__file__).parent / 'common_old.onnx',
                output / 'common_old.onnx')
print('模型目录:', output.resolve())
print('运行库目录:', (Path(onnxruntime.__file__).parent / 'capi').resolve())
PY
```

在打印出的运行库目录找到 `libonnxruntime.so` 或带版本号的文件，在菜单中填写其完整路径。保留同目录的配套 `.so` 文件。模型与字符表可以直接使用 Windows 导出的文件。

## 输入输出约定

字符配置包含 `charset`、`word=false`、`image=[-1,64]`、`channel=1`。字符表共 8210 项，第零项为空字符串，不能替换成十个数字。

输入 `input1` 为 float32 `[1,1,64,W]`：RGB 图片按比例缩放到高 64，宽度向下取整，Lanczos3 插值后转灰度、除以 255。Rust 与 Pillow 插值不保证逐像素相同；现有 14 张最终预测已逐一核对一致。

实际输出 `[T,1,8210]`。完整类别 argmax，CTC 合并相邻重复后移除 blank，保留前导零和被 blank 分开的重复数字。模型静态输出元数据有误，因此检查实际推理张量。

仅接受四位 ASCII 数字，不截断、补零或替换字符。格式正确仍可能读错，需结合学校响应和预约记录核验。

## 已验证范围

开发阶段的 14 张标注图片中，模型识别正确 11 张。样本量较小，不能用这个比例保证线上识别成功率。

来源：[ddddocr 项目](https://github.com/sml2h3/ddddocr)、[ddddocr 1.6.1](https://pypi.org/project/ddddocr/1.6.1/)。
