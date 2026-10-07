# Fleqi 首版能力台账

版本：1.0 · 日期：2026-09-15。状态：全部条目纳入首版 macOS，外部条件不改变实现版本。

本台账落实 [产品需求](requirements.md) 的 FR-CAP、FR-RUN、FR-TOOLS、FR-POLICY；界面见 [界面设计](ui-design.md)，实现分工见 [架构](architecture.md)，排期见 [开发计划](development-plan.md)。

P0 工程准备与产品实现分开；首版范围和默认值不因排期改变。当前实现与验证见[实施状态](status.md)。

## 1. 台账规则

六类 UI 基础能力共 30 项；历史目录共 10 类、105 个操作意图，以下逐条保留 legacy_id 作为来源键。操作可能映射到同一新能力或同一能力的不同参数组合；来源键仍各占一行，不合并丢失。历史 shell 文本不作为新实现或验收合同，本文不保留其片段。

所有 CAP-* 都属于 macOS 首版，不存在“待确认”“可选首发”或以缺硬件为理由的延期分类。Windows/Linux 后续在同一能力合同下增加实现和平台验收。实时语音下一版；文件转写、SRT/VTT 字幕、OCR 全部首版。

Windows 11 x64 本轮开放 21 项：CAP-FILE-001 至 CAP-FILE-008、CAP-FILE-012、CAP-ZIP-001 至 CAP-ZIP-003、CAP-IMAGE-001 至 CAP-IMAGE-004、CAP-PDF-001 至 CAP-PDF-005。图片范围为 PNG/JPEG/WebP，PDF 为基础页面操作和结构压缩。其余 Windows 条目保持“暂未开放”，表单、AI 原生计划、执行器及历史重试不得绕过平台判定；普通用户终端和经策略确认的 AI 脚本仍使用其已有执行规则。此清单不是完整 Windows 能力验收，也不改变上述 macOS 首版范围。

Windows 回收站必须使用原生 Shell 回收操作，不得失败后改用永久删除；恢复以系统回收站真实往返为验收依据，不编造内部恢复路径。ZIP 解压必须拒绝盘符/绝对路径、父目录跳转、反斜杠穿越、ADS，以及输出路径中的符号链接或 junction。无法解码的图片直接说明支持范围，不调用 macOS 的 sips。

每行的 AC-CAP-* 是独立验收项，必须同时满足本节公共合同和该行的具体断言。30 个基础 AC 加 105 个来源映射 AC，共 135 项验收。共用 CAP 不意味着可跳过其不同参数/格式场景。

### 1.1 入口与模块

所有执行能力统一使用 UI-COMPOSER → UI-RUN-DETAIL / UI-TERMINAL。用户以自然语言发起时走 AI 计划和当前 aiPolicy；用户以 ! 或终端面板输入时保持手动 PTY 语义。目录管理入口为 UI-CAPABILITY-LIBRARY；依赖管理入口为 UI-TOOLS。

application 编排请求与生命周期，catalog 提供能力和参数，planner 形成执行计划，policy 判定 AI 确认，process 管理工具子进程，terminal 管理持续 PTY，model 连接用户配置服务，storage 保存记录，platform 提供系统能力。UI 不直接实现文件、网络、工具安装或原生副作用。

### 1.2 平台与外部条件

表中 M 统一表示“macOS 首版完整实现”。表内条件是使用该能力的真实前置条件，必须由首版检测、展示并提供下一步；不是未实现占位。条件满足时必须完成成功路径，条件不满足时必须完成相应失败/引导验收。

账号类条件只指用户已有的 Git 远端、消息服务等外部账号，不是 Fleqi App 登录。无需订阅、试用、商业许可证或 Fleqi 自营模型代理。涉及联网的数据操作须明确目的地、参数及网络结果。

### 1.3 公共输入输出合同

| 公共 ID | 合同 |
|---|---|
| AC-COMMON-001 | 文件输入绑定提交时显示的 FinderContext；记录目录、选择顺序、完整路径与必要属性。虚拟视图无有效目录时要求选择目录，不自行猜测。 |
| AC-COMMON-002 | 参数不足时补齐必要参数；不能从历史示例代填密码、收件人、分支、城市、路径、时间或金额。来源 ID 中保留的示例文字不构成运行参数。 |
| AC-COMMON-003 | 生成文件默认保留原件，输出默认在已显示的有效输入目录；用户指定位置优先。重名默认生成明确的不冲突名称，不静默覆盖。明确原地修改意图按当前策略处理。 |
| AC-COMMON-004 | 批处理明确排序、作用范围、每项结果和部分成功；文件夹递归、隐藏项、链接跟随、覆盖及删除不能靠含糊的“全部”隐式扩大。 |
| AC-COMMON-005 | 文档与媒体输出必须真实存在、可由独立解析器读取，并满足格式、数量、尺寸/时长/页数及内容断言；不能只依赖退出码或模型结论。 |
| AC-COMMON-006 | 路径作为结构化参数安全传递；覆盖空格、Unicode、引号、换行和 shell 特殊字符。手动 shell 内容维持用户原意，不与宿主生成参数混用。 |
| AC-COMMON-007 | 缺工具通过 UI-TOOLS 或任务依赖流程安装；AI 安装/卸载服从 aiPolicy，安装完成重新验证。工具版本、来源、校验和平台信息由受管目录确定。 |
| AC-COMMON-008 | 每项执行可取消或明确说明已发出的不可撤回系统动作；取消记录已产生的改动，不宣称通用回滚。执行/安装进度区分确定与不确定状态。 |
| AC-COMMON-009 | 输出结果、错误、依赖、应用管理的真实运行命令/动作和文件入口进入 Run/Session 记录；应用管理凭据与可识别秘密参数脱敏，原始 PTY 输入字节不记录。不承诺识别任意终端输出中的全部未知秘密。 |
| AC-COMMON-010 | 默认 readOnlyAutoConfirmChanges 仅可信已知只读 AI 操作免确认，修改与未知需确认；yolo 全部 AI 免确认。手动 ! 与终端面板均直通，不新增脚本白名单。 |
| AC-COMMON-011 | 条件不足必须具体说明缺少什么；权限/索引/账号导致结果不完整时明确展示范围，不伪装为完整空结果或成功。 |
| AC-COMMON-012 | 新 ! 命令等待提交时的目标目录同步；等待中 Finder 再换目录则撤销自动投递、保留草稿并提示重新提交。终端原始输入仍可继续控制现有程序。新 AI Run 用最新显示的 Finder 快照，旧 Run 保持原快照。 |

### 1.4 逻辑依赖目录

逻辑依赖名称是能力与工具目录之间的契约，不强制单一旧 CLI。具体受管实现、版本、签名和构建由架构与工具目录落实；每个首版依赖必须有可安装或内置的真实实现。

| 依赖 ID | 必须提供的能力与实现范围 |
|---|---|
| DEP-FS | Rust 文件/目录读取、复制、移动、改名、哈希、遍历和基础文本能力；回收站等原生行为由 platform 适配。 |
| DEP-ZIP | 内置 Rust ZIP 创建、目录、读取与普通 ZIP 解压；明确路径边界及重名行为。 |
| DEP-IMAGE | PNG/JPG/WebP 解码、编码、尺寸、旋转与质量控制；内置 Rust 图像工具可直接实现。Apple HEIF/HEIC 在 macOS 经系统 sips 解码，在 Linux 经 libheif 解码，再编码为 PNG/JPG/WebP。 |
| DEP-IMAGE-EXT | 受管图像处理适配，覆盖裁切、合成、元数据、透明、GIF、ICNS/ICO 等；可组合 Rust 与 ImageMagick 类实现。 |
| DEP-MEDIA | FFmpeg/ffprobe 类受管适配，提供媒体检查、转码、提轨、裁剪、旋转和裁切。 |
| DEP-PDF | qpdf 类结构处理适配，提供合并、拆分、提页、旋转、结构压缩和加解密。 |
| DEP-PDF-CONTENT | PDF 元数据、正文和嵌入图片适配，可组合 PDF 库与 Poppler 类工具。 |
| DEP-DOCX | Rust/OOXML 基础 DOCX 创建及正文读取；不执行文档宏。 |
| DEP-DOC-IMPORT | 富文本文档正文转换适配，必须覆盖 DOC、DOCX、DOCM、ODT、RTF、RTFD；可组合 OOXML、Pandoc 和受管办公文档转换器，不以某一工具缺格式为理由跳过。 |
| DEP-OCR | Tesseract 类 OCR 适配、语言模型和必要的图像解码；首版工具目录可安装完整组合。 |
| DEP-ASR | whisper.cpp 类本地音视频转写、时间戳及语言模型；首版提供真实可安装组合，支持文本/SRT/VTT 输出。 |
| DEP-MODEL | 用户配置的 API、自定义地址或本地服务，负责自然语言理解与摘要；无 Fleqi 自营代理前提。 |
| DEP-PLATFORM | macOS Finder、索引/元数据、应用启动、系统设置、卷、打印、硬件和权限适配。 |
| DEP-TERM | 当前 Session 的持续 PTY 与终端应用启动接口；遵守安全目录同步。 |
| DEP-GIT | 可检测、安装并调用的 Git 及远端认证；不替用户生成账号或凭据。 |
| DEP-NET | 网络探测、HTTP(S) 下载与服务调用适配；显示真实目的地和协议。 |
| DEP-WEATHER | 天气服务适配，首版至少提供一条不依赖 Fleqi 账号的可用服务路径，并支持地点与单位参数。 |
| DEP-MATH | 本地数值、单位和时间换算，明确精度、取整和输入范围。 |
| DEP-TOOLS | Fleqi 工具目录、受管安装器和系统包管理器适配；记录所有权及实际安装状态。 |
| DEP-MESSAGES | macOS 消息应用自动化/服务适配；已有可用消息账号、有效收件对象和系统授权。 |

### 1.5 首版格式与参数基线

这些值是内置能力的设计合同；用户可明确提供该能力允许的参数，手动 shell 保持自己的语义。工具目录必须提供能够满足本表的实现，不能仅写“视工具支持而定”。

| 范围 | 首版确定的格式、参数与结果规则 |
|---|---|
| 输出名称 | 生成类任务默认在界面明示的输入目录建立新文件；沿用主文件名加操作后缀，冲突追加 ` (1)`、` (2)` 等递增编号，扩展名最后保留。组合/多输入输出先由用户核对目标；文件夹名不擅自按扩展名拆分。 |
| 范围与排序 | 默认仅显式选中项；明确的文件夹递归能力才递归。默认不跟随符号链接，不隐式处理隐藏项；需要时作为显式参数。批量编号/图片序列默认按文件名自然排序，预览列出最终顺序并允许用户调整。 |
| TXT/Markdown | 创建默认 UTF-8 无 BOM、LF；读取支持 UTF-8、带 BOM 的 UTF-16LE/BE 和用户显式选择的 GB18030。无 BOM 先严格按 UTF-8，失败要求选择编码，不无声替换非法字节。Markdown 读取保留源文。 |
| 字词统计 | `words` 定义为空白分隔的非空文本段，结果标注该规则；另支持 `characters` 的 Unicode 字符数及是否计空白参数。富文本先抽正文再用同一规则，不把格式标记计入正文。 |
| DOCX | 基础创建包含段落、1–3 级标题、单层有序/无序列表；正文提取包含主文档段落和表格单元格，按文档顺序排列。页眉页脚、批注、脚注、图片内文字不包含在“正文”默认范围，结果说明这一点；不运行宏。 |
| 富文本导入 | DOC/DOCX/DOCM/ODT/RTF/RTFD 均有首版转换路径；提取正文，不承诺原版面重建。RTFD 作为文档包识别；宏和外链资源不执行，已知加密输入先请求有效密码或明确不支持的加密格式。 |
| 基础图片 | PNG、JPEG、静态 WebP 之间的六个转换方向；尺寸保持原样。JPG 质量默认 90、WebP 默认 82，允许 1–100；透明转 JPG 默认白色背景且在预览显示，可指定其他颜色。 |
| 图片扩展 | 至少读取 PNG/JPEG/WebP/HEIC/TIFF/GIF；动画输入涉及输出静态图时必须明确选帧，不静默丢帧。缩放默认保持比例且不放大；旋转基础角度为 90/180/270；GIF 生成支持逐帧时长和循环次数，顺序先预览。 |
| 音频 | MP3、M4A（AAC）、WAV（PCM）六个转换方向。默认尽量保持输入声道/采样率；用户可选 44.1/48 kHz、单/双声道。目标 MP3 默认 192 kbps，AAC 默认 192 kbps，WAV 默认 16-bit PCM，参数变化在计划中显示。 |
| 视频 | 首版输入覆盖 MP4/MOV/MKV/WebM 中 H.264、HEVC、VP8、VP9 视频及 AAC/MP3/Opus/PCM 音轨；目标 MP4 默认为 H.264 + AAC、yuv420p，保持源尺寸和帧率。缺轨/不支持编码明确失败；不以改文件扩展名冒充转换。 |
| 裁剪 | 精确模式允许重编码，误差不超过一个输出视频帧或一个音频编解码帧；无重编码模式保留压缩流，受关键帧约束，结果报告实际起止，不承诺同样精度。起点必须非负，终点晚于起点且不超过可探测时长。 |
| OCR/文件转写 | OCR 首版支持简体中文/英文及混合文本；转写支持中文/英文音视频和自动识别语言，输出 TXT/SRT/VTT。语言模型作为受管依赖，原文件保持；字幕时间非负、起点早于终点且按时间排序。 |
| PDF | 结构操作保留页面视觉；合并顺序、提页页码、重复页/乱序页均以预览为准；旋转允许 90/180/270。加密支持工具可验证的 AES-128/AES-256，默认 AES-256；所需用户口令/权限口令为秘密参数，不能使用固定示例口令。 |
| ZIP | 普通 ZIP 创建、列出与解压；目录和 Unicode 路径保留。解压目标为独立目录，条目不得越界；链接条目不自动跟随写入。加密包不属于基础 ZIP 解压，明确说明，不将空目录记成成功。 |

金额/数值、日期、裁切范围、网络目标、Git 分支/远端、收件人和密码等无用户上下文的必要参数必须补齐。上述媒体质量和文件名后缀是可见的设计默认值，不把历史示例中的数值或品牌当作用户输入。

编码/解码及加密适配参考 [FFmpeg 编码器文档](https://ffmpeg.org/ffmpeg-codecs.html)和 [qpdf 加密文档](https://qpdf.readthedocs.io/en/stable/encryption.html)；安装清单须探测实际构建包含的实现，默认质量参数由 Fleqi 显式传入，不依赖工具隐含默认值。

## 2. 六类基础能力：30 项

| CAP ID | 操作及参数 | 输入 | 输出与边界 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| CAP-FILE-001 | 创建文本文件；名称、内容、编码、位置 | 有效目录及正文 | 新文件；缺内容可明确创建空文件；不误覆盖 | DEP-FS | M；目录可写 | AC-CAP-001：在含空格/Unicode 路径创建文件，读回正文与编码正确。 |
| CAP-FILE-002 | 创建文件夹；名称、父目录、是否创建中间层 | 有效父目录 | 新目录；同名已有目录返回真实状态 | DEP-FS | M；父目录可写 | AC-CAP-002：创建单层/显式多层目录；同名冲突不破坏已有内容。 |
| CAP-FILE-003 | 复制；源列表、目标、冲突规则、递归范围 | 文件与文件夹，单项/多项 | 新副本，原件保留；逐项报告 | DEP-FS | M；源可读、目标可写 | AC-CAP-003：复制混合选区，文件哈希及目录结构一致，原件存在。 |
| CAP-FILE-004 | 移动；源列表、目标、冲突规则 | 文件与文件夹，单项/多项 | 目标正确；跨卷失败与部分完成可见 | DEP-FS | M；源/目标权限可用 | AC-CAP-004：同卷/跨卷移动后内容一致；模拟失败不误删唯一副本。 |
| CAP-FILE-005 | 改名；模板、前后缀、日期、大小写及扩展名规则 | 一项或多项文件/文件夹 | 预览与实际名称对应；检查大小写冲突 | DEP-FS | M；父目录可写 | AC-CAP-005：批量改名含无扩展名/同名冲突，结果与预览一致。 |
| CAP-FILE-006 | 批量编号；起始值、步长、位数、位置、排序 | 两项及以上，或显式单项 | 按确定顺序产生唯一目标名 | DEP-FS | M；父目录可写 | AC-CAP-006：不同原选区顺序下按指定排序得到相同编号结果。 |
| CAP-FILE-007 | 整理；按类型/日期/显式分类规则、范围、目标 | 有效目录或明确选区 | 分类目录与移动计划；不隐式扩大递归范围 | DEP-FS | M；所需读写权限 | AC-CAP-007：类型、日期各执行一组样本，未纳入范围文件保持不变。 |
| CAP-FILE-008 | 移入回收站；源列表 | 一项或多项文件/文件夹 | 原位置移除且可从平台回收站恢复；不等价永久删除 | DEP-FS、DEP-PLATFORM | M；回收站可用、权限满足 | AC-CAP-008：真实送入回收站并验证恢复；不支持位置明确失败。 |
| CAP-ZIP-001 | ZIP 打包；选区、归档名、目标、目录根规则 | 文件/文件夹，单项或多项 | 普通 ZIP，保留约定相对结构 | DEP-ZIP | M；目标可写 | AC-CAP-009：重新解压归档，文件哈希和目录结构对应输入。 |
| CAP-ZIP-002 | 查看 ZIP 内容；显示路径/大小/条目类型 | 一个 ZIP | 条目列表，无需全量解压 | DEP-ZIP | M；归档可读 | AC-CAP-010：目录、空文件、Unicode 名称全部列出且数目正确。 |
| CAP-ZIP-003 | 普通 ZIP 解压；目标、重名处理 | 普通未加密 ZIP | 安全目标目录与逐项结果；加密/损坏归档明确反馈 | DEP-ZIP | M；目标可写 | AC-CAP-011：正常包完整解压；越界路径条目不写到目标之外。 |
| CAP-IMAGE-001 | PNG/JPG/WebP 转换；目标格式、质量、透明背景处理。输入可包含 Apple HEIF/HEIC。 | PNG/JPG/WebP，以及 HEIF/HEIC 单张或多张 | PNG/JPG/WebP 互转，HEIF/HEIC 转为这三种格式；JPG 无透明时背景参数明确 | DEP-IMAGE | M；图像可解码。HEIF 在 macOS 使用 sips，在 Linux 使用 libheif | AC-CAP-012：逐方向验证格式、尺寸、透明/背景及原件保留。HEIF 样本转为 PNG 后尺寸与像素一致，原件保留。 |
| CAP-IMAGE-002 | 缩放/缩略图；宽/高/边界框、比例、是否放大 | PNG/JPG/WebP，扩展格式按适配 | 新图像；按指定约束保持比例或明确拉伸 | DEP-IMAGE、DEP-IMAGE-EXT | M；输入格式有解码器 | AC-CAP-013：固定宽、固定高、边界框及批量结果尺寸正确。 |
| CAP-IMAGE-003 | 旋转；90/180/270 度、方向 | 图像单张/多张 | 新图像；方向、尺寸、透明度正确 | DEP-IMAGE | M；图像可解码 | AC-CAP-014：非方形带方向标识样本验证各角度结果。 |
| CAP-IMAGE-004 | JPG 压缩；质量档/数值、元数据策略 | JPG 单张/多张 | 新 JPG；可解码，不保证每张都变小；报告大小变化 | DEP-IMAGE | M；JPG 可解码 | AC-CAP-015：检查质量参数生效、原件保留及不能缩小时的真实说明。 |
| CAP-MEDIA-001 | 音频转换；目标格式、采样率、声道、质量 | MP3/M4A/WAV 单项/多项 | 三格式间六个转换方向；目标格式参数明确 | DEP-MEDIA | M；相应解码/编码器可用 | AC-CAP-016：六方向输出可播放，时长/声道在声明容差内。 |
| CAP-MEDIA-002 | 视频格式转换（MP4/MOV/MKV）；视频/音频编码、质量、轨道选择 | 支持解码的媒体；至少 MP4/MOV/MKV/WebM | 默认 MP4，可选 MOV/MKV；新文件校验成功后按设置保留原件或移入回收站，错误格式不伪成功 | DEP-MEDIA | M；所需编码器可用 | AC-CAP-017：矩阵代表样本转 MP4，逐轨验证编码与音画同步。 |
| CAP-MEDIA-003 | 提取音频；音轨、输出格式 | 带音轨的视频 | 音频文件；多音轨需指定/明确选择，无音轨可读失败 | DEP-MEDIA | M；音轨与编码器可用 | AC-CAP-018：单轨/多轨正确提取，无音轨样本不生成伪空成功文件。 |
| CAP-MEDIA-004 | 裁剪；起止/时长、精确或无重编码模式 | 音频/视频 | 目标片段；无损模式受关键帧/容器约束时解释实际边界 | DEP-MEDIA | M；有效时段与编码器 | AC-CAP-019：精确模式验证时间范围；无重编码模式验证无转码及边界说明。 |
| CAP-PDF-001 | 合并；文件顺序、输出名 | 两个及以上普通未加密 PDF | 合并 PDF，页序与输入顺序一致 | DEP-PDF | M；所有输入可读 | AC-CAP-020：不同页数和尺寸文档合并后逐页内容与总页数正确。 |
| CAP-PDF-002 | 拆分；逐页或页组、命名规则 | 普通未加密 PDF | 多个 PDF，页数组合覆盖请求范围 | DEP-PDF | M；目标可写 | AC-CAP-021：逐页及页组拆分后重组内容与请求对应。 |
| CAP-PDF-003 | 提页；页号/范围、顺序、输出名 | 普通未加密 PDF | 指定页组成的新 PDF；越界页拒绝执行 | DEP-PDF | M；页参数有效 | AC-CAP-022：乱序/重复页按显式规则输出，越界不生成假结果。 |
| CAP-PDF-004 | 旋转页；页范围、角度、方向 | 普通未加密 PDF | 新 PDF；非目标页保持 | DEP-PDF | M；页参数有效 | AC-CAP-023：选页旋转，页数及未选页面内容不变。 |
| CAP-PDF-005 | 结构压缩；输出名、保真约束 | 普通未加密 PDF | 不主动栅格化或降低视觉质量；可报告无进一步缩小空间 | DEP-PDF | M；输入结构可读 | AC-CAP-024：页面渲染/文本保持；体积不降时不报虚构节省。 |
| CAP-TEXT-001 | 读取 TXT；编码、读取范围 | TXT | 正文与编码信息，过大文件显示截取/分页范围 | DEP-FS | M；可读、编码支持 | AC-CAP-025：UTF-8 与支持的其它编码读回正确，非法编码明确报错。 |
| CAP-TEXT-002 | 创建 TXT；正文、编码、换行、位置 | 用户正文/明确空内容 | 新 TXT | DEP-FS | M；目录可写 | AC-CAP-026：读回文字、换行、编码与请求一致。 |
| CAP-TEXT-003 | 读取 Markdown；编码、范围 | MD/Markdown | 保留原文，不依赖渲染成功 | DEP-FS | M；可读文本 | AC-CAP-027：标题、列表、代码块原文完整，读取不改文件。 |
| CAP-TEXT-004 | 创建 Markdown；正文、位置 | 用户正文 | 新 Markdown 文件 | DEP-FS | M；目录可写 | AC-CAP-028：原样读回格式标记和 Unicode 内容。 |
| CAP-TEXT-005 | 创建简单 DOCX；正文、标题、基础列表、输出名 | 结构化正文 | DOCX；不承诺复杂排版复刻，支持元素写清 | DEP-DOCX | M；目录可写 | AC-CAP-029：独立阅读器能打开，段落/标题/列表和正文正确。 |
| CAP-TEXT-006 | 提取 DOCX 正文；范围、输出方式 | DOCX | 主文档正文；表格单元格按文档顺序包含，脚注/图片文字等范围明示 | DEP-DOCX | M；未加密且结构有效 | AC-CAP-030：段落及表格正文顺序正确，非正文遗漏范围明确。 |

## 3. 历史操作意图逐项映射：105 项

### 3.1 Video and audio：10 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| media.info.duration | CAP-MEDIA-005 | 音/视频文件；单位与精度 | 真实时长及单位 | DEP-MEDIA | M；媒体可探测 | AC-CAP-031：与独立探测结果一致，未知时长不显示为零。 |
| video.info.resolution | CAP-MEDIA-006 | 视频；视频轨道 | 宽、高及旋转显示关系 | DEP-MEDIA | M；有视频轨 | AC-CAP-032：多轨及带旋转元数据样本显示正确轨道分辨率。 |
| video.info.bitrate | CAP-MEDIA-007 | 视频；整体或轨道、单位 | 码率及其来源/估算标记 | DEP-MEDIA | M；媒体可探测 | AC-CAP-033：已声明码率准确，不能探测时说明而非伪造。 |
| video.info.codec | CAP-MEDIA-008 | 视频；轨道选择 | 视频编码名称与必要参数 | DEP-MEDIA | M；有视频轨 | AC-CAP-034：不同编码样本及多轨选择返回对应编码。 |
| media.transcribe.text | CAP-MEDIA-009 | 音/视频；语言、转写模型、输出名 | 纯文本转写 | DEP-ASR、DEP-MEDIA | M；语言模型已安装、有音轨 | AC-CAP-035：已知短音频形成可读正文，静音和无音轨状态正确。 |
| media.subtitles.srt | CAP-MEDIA-010 | 音/视频；语言、分段规则、输出名 | 带时间戳的 SRT | DEP-ASR、DEP-MEDIA | M；转写模型与音轨可用 | AC-CAP-036：SRT 可解析，序号/时段合法且字幕对应音频。 |
| media.subtitles.vtt | CAP-MEDIA-011 | 音/视频；语言、分段规则、输出名 | WebVTT 字幕 | DEP-ASR、DEP-MEDIA | M；转写模型与音轨可用 | AC-CAP-037：VTT 头部、时间戳和字幕正文可被独立播放器读取。 |
| media.trim.lossless | CAP-MEDIA-004 | 音/视频；起始点、时长、无重编码模式 | 无重编码片段及实际裁剪边界 | DEP-MEDIA | M；流复制与容器条件满足 | AC-CAP-038：验证未重新编码；不能精准对齐时说明边界，不偷偷改为有损。 |
| video.rotate.clockwise-90 | CAP-MEDIA-012 | 视频；旋转角度/方向、编码参数 | 旋转后的新视频 | DEP-MEDIA | M；视频可解码/编码 | AC-CAP-039：带方向标记视频旋转正确，音轨保留且音画同步。 |
| video.crop.16-9-to-16-10 | CAP-MEDIA-013 | 视频；目标宽高比、裁切锚点 | 按目标比例裁切的新视频 | DEP-MEDIA | M；裁切区域有效 | AC-CAP-040：目标比例正确，不拉伸画面；不合法区域先报错。 |

### 3.2 Images：18 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| metadata.image-resolution | CAP-IMAGE-005 | 图像；像素/方向信息 | 像素宽高与显示方向 | DEP-IMAGE、DEP-IMAGE-EXT | M；格式可解码 | AC-CAP-041：不同方向元数据样本的存储/显示尺寸明确。 |
| image.ocr.text | CAP-IMAGE-006 | 图像；语言、识别范围、输出名 | OCR 正文文件/可查看文本 | DEP-OCR | M；语言模型已安装 | AC-CAP-042：固定多语言样本能提取预期关键文本，无文字不编造。 |
| image.rotate.right-90 | CAP-IMAGE-003 | 图像；向右旋转角度 | 旋转后的新图像 | DEP-IMAGE、DEP-IMAGE-EXT | M；格式可解码 | AC-CAP-043：向右 90 度样本方向、尺寸和透明通道正确。 |
| image.resize.1080-tall | CAP-IMAGE-002 | 图像；目标高度、比例、是否放大 | 指定高度的新图像 | DEP-IMAGE、DEP-IMAGE-EXT | M；高度为正整数 | AC-CAP-044：目标高度参数生效，宽度按比例取整且有说明。 |
| image.thumbnail.200 | CAP-IMAGE-002 | 图像；缩略图边界框、填充/不填充 | 不超出边界框的缩略图 | DEP-IMAGE、DEP-IMAGE-EXT | M；边界尺寸有效 | AC-CAP-045：横竖图均适配指定框，默认不变形。 |
| image.strip-exif | CAP-IMAGE-007 | 图像；需删除的元数据范围 | 新图像与移除字段摘要 | DEP-IMAGE-EXT | M；适配器支持该格式元数据 | AC-CAP-046：独立读取确认目标 EXIF 已移除，图像仍可解码。 |
| image.batch-resize.folder-1024-wide | CAP-IMAGE-002 | 目录；目标宽度、格式集合、递归/覆盖范围 | 每张图片的新缩放文件 | DEP-IMAGE、DEP-IMAGE-EXT | M；目录可读写 | AC-CAP-047：JPG/JPEG/PNG/HEIC/TIFF/GIF/WebP 代表样本逐项处理，范围外不改。 |
| image.info.true-file-type | CAP-FILE-009 | 扩展名可疑的文件；检测模式 | 基于内容的真实类型与扩展名差异 | DEP-FS、DEP-IMAGE-EXT | M；文件可读 | AC-CAP-048：改成 JPG 扩展名的 PNG 被识别为 PNG，不能只读后缀。 |
| image.crop.50px-each-side | CAP-IMAGE-008 | 图像；四边裁切量或区域 | 新裁切图像 | DEP-IMAGE-EXT | M；剩余区域大于零 | AC-CAP-049：各边参数与输出尺寸相符，越界输入不产生空成功结果。 |
| image.remove-white-background | CAP-IMAGE-009 | 图像；背景颜色、容差、输出格式 | 支持透明通道的新图像 | DEP-IMAGE-EXT | M；透明输出格式可编码 | AC-CAP-050：目标颜色按容差透明化，选择 JPG 时要求明确透明替代策略。 |
| image.icns.single | CAP-IMAGE-010 | 单张图像；尺寸集合、输出名 | 可读 ICNS 图标 | DEP-IMAGE-EXT | M；ICNS 编码可用 | AC-CAP-051：独立读取各图层尺寸，临时中间文件清理不影响用户原图。 |
| image.ico.single | CAP-IMAGE-011 | 单张图像；尺寸集合、输出名 | 多尺寸 ICO | DEP-IMAGE-EXT | M；ICO 编码可用 | AC-CAP-052：独立读取各尺寸及透明度，文件扩展名与实际格式一致。 |
| image.gif.from-images | CAP-IMAGE-012 | 多张图像；顺序、帧时长、循环次数 | 动画 GIF | DEP-IMAGE-EXT | M；至少两帧或明确单帧 | AC-CAP-053：帧数、顺序、时长和循环参数均正确。 |
| image.blur | CAP-IMAGE-013 | 图像；模糊半径/强度 | 新模糊图像 | DEP-IMAGE-EXT | M；参数合法 | AC-CAP-054：目标强度生效，尺寸与输入一致，原件保留。 |
| image.border.10px | CAP-IMAGE-014 | 图像；边框宽度、颜色 | 带边框的新图像 | DEP-IMAGE-EXT | M；参数合法 | AC-CAP-055：边框颜色正确，输出宽高按边框数值增加。 |
| image.grid.3x3 | CAP-IMAGE-015 | 多图；行列数、顺序、间距/背景 | 拼图图像 | DEP-IMAGE-EXT | M；数量与网格规则满足 | AC-CAP-056：指定网格每格对应输入顺序，空格补齐规则明确。 |
| image.tint-blue | CAP-IMAGE-016 | 图像；着色颜色、比例/强度 | 新着色图像 | DEP-IMAGE-EXT | M；参数合法 | AC-CAP-057：不同颜色与强度可区分，透明度及尺寸按合同保留。 |
| image.overlay.substage | CAP-IMAGE-017 | 图像；用户文字、字体、大小、颜色、位置 | 文字覆盖的新图像 | DEP-IMAGE-EXT | M；指定字体可用或用户选择替代 | AC-CAP-058：中文与英文文字实际渲染，文字参数不得来自来源键；缺字体反馈准确。 |

### 3.3 Text：7 项

文本输入覆盖 TXT、Markdown、CSV/TSV、JSON、XML、YAML、HTML、CSS、JS/TS、Swift、Python、Ruby 与 shell 等可解码文本。RTF 属结构文档时使用正文适配器，不按格式标记统计成用户正文。

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| text.word-count | CAP-TEXT-007 | 文本文件；统计单位/语言规则、多文件汇总方式 | 每文件及总字词数，标明计数规则 | DEP-FS、DEP-DOC-IMPORT | M；编码/格式可读 | AC-CAP-059：英文空白分词和中文字符/分词规则各有固定样本断言，不混淆单位。 |
| text.word-count.converted-document | CAP-TEXT-008 | DOC/DOCX/DOCM/ODT/RTFD/RTF；统计规则 | 正文字词数，不计入格式标记 | DEP-DOC-IMPORT | M；转换器支持输入、无未知密码 | AC-CAP-060：六类代表文档正文统计与提取文本一致，宏不执行。 |
| text.summarize.plain-text | CAP-TEXT-009 | 文本；摘要语言、长度、范围 | 基于输入内容的摘要 | DEP-FS、DEP-MODEL | M；文本可读、模型已配置 | AC-CAP-061：摘要包含样本关键事实，截取/无法读取范围明确，不修改原文。 |
| text.summarize.converted-document | CAP-TEXT-010 | DOC/DOCX/DOCM/ODT/RTFD/RTF；摘要参数 | 正文摘要及处理范围 | DEP-DOC-IMPORT、DEP-MODEL | M；正文可提取、模型可用 | AC-CAP-062：各格式正文进入摘要，转换失败不被模型补造为内容。 |
| text.extract-readable | CAP-TEXT-011 | 文档；格式、正文范围、编码、输出位置 | 可读正文文本 | DEP-DOC-IMPORT | M；明确支持的输入格式可读取 | AC-CAP-063：六类富文本文档及 DOCX 正文提取通过，图片文字需求转到 OCR 而非伪识别。 |
| text.open.textedit | CAP-SYSTEM-001 | 文档；目标文本编辑应用 | 在指定/默认编辑器打开文件的系统结果 | DEP-PLATFORM | M；编辑应用已安装、可读文件 | AC-CAP-064：默认文本编辑器和用户指定已安装编辑器都打开正确路径，缺应用可读报错。 |
| text.create.readme | CAP-TEXT-002 | 当前目录；文件名、正文、编码 | 新文本说明文件 | DEP-FS | M；目录可写 | AC-CAP-065：参数化名称与正文创建正确，已有同名文件不静默覆盖。 |

### 3.4 PDFs：12 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| pdf.info.page-count | CAP-PDF-006 | PDF；必要时用户提供密码 | 页数 | DEP-PDF、DEP-PDF-CONTENT | M；PDF 可解读 | AC-CAP-066：不同页数样本正确，无法解密时不报零页。 |
| pdf.info.author | CAP-PDF-007 | PDF；字段范围 | 作者元数据或确实未设置 | DEP-PDF-CONTENT | M；元数据可读 | AC-CAP-067：有作者/无作者各有正确结果，不从正文猜测作者。 |
| pdf.summarize | CAP-PDF-008 | PDF；页范围、摘要语言/长度 | 正文摘要，扫描页按需 OCR | DEP-PDF-CONTENT、DEP-OCR、DEP-MODEL | M；正文或 OCR 可用、模型配置 | AC-CAP-068：文本 PDF 和扫描 PDF 均能摘要，OCR/页范围缺失明确标注。 |
| pdf.merge.selected | CAP-PDF-001 | 多 PDF；顺序、输出名 | 合并 PDF | DEP-PDF | M；基础路径为未加密输入 | AC-CAP-069：输入选择顺序改变时输出页序相应改变，总页数正确。 |
| pdf.optimize | CAP-PDF-005 | PDF；输出名、结构保真策略 | 结构优化 PDF 与真实大小差异 | DEP-PDF | M；结构可读 | AC-CAP-070：无主动视觉降质；不能缩小时显示真实结果，不继承旧有损实现。 |
| pdf.split-pages | CAP-PDF-002 | PDF；逐页命名、输出目录 | 每页单独 PDF | DEP-PDF | M；输入可读、目标可写 | AC-CAP-071：输出数量等于请求页数，每个文件对应正确页面。 |
| pdf.extract-images | CAP-PDF-009 | PDF；页范围、原始格式/转换格式、输出目录 | 嵌入图片集合及来源页信息 | DEP-PDF-CONTENT | M；页面可解析 | AC-CAP-072：已知嵌入图数量/内容对应；无图片给确定空结果。 |
| pdf.remove-metadata | CAP-PDF-010 | PDF；需移除的文档信息/XMP 字段 | 新 PDF 与实际移除字段 | DEP-PDF、DEP-PDF-CONTENT | M；结构可处理 | AC-CAP-073：目标字段经独立读取消失，页面正文与页数保留。 |
| pdf.extract-page-2 | CAP-PDF-003 | PDF；参数化页号/范围、输出名 | 提取页 PDF | DEP-PDF | M；页号有效 | AC-CAP-074：任意用户指定页正确提取，不把来源键数字作为固定参数。 |
| pdf.rotate-page-2-clockwise | CAP-PDF-004 | PDF；参数化页号/范围、角度/方向 | 旋转后的新 PDF | DEP-PDF | M；页号有效 | AC-CAP-075：所选页按指定方向旋转，其它页不变。 |
| pdf.remove-password | CAP-PDF-011 | 加密 PDF；用户提供的有效密码、输出名 | 解密后的新 PDF | DEP-PDF | M；正确密码及工具支持的加密格式 | AC-CAP-076：正确密码生成可无密码打开的文件；错误密码失败且不记录秘密。 |
| pdf.add-password | CAP-PDF-012 | PDF；用户口令/权限口令、权限、加密设置 | 加密后的新 PDF | DEP-PDF | M；参数与加密格式受支持 | AC-CAP-077：无密码不能打开，正确密码可读，权限参数生效；日志不含口令。 |

### 3.5 Zip archives：5 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| archive.zip.selected-files | CAP-ZIP-001 | 选中文件/文件夹；归档名、目录规则 | ZIP 归档 | DEP-ZIP | M；输入可读、目标可写 | AC-CAP-078：多个文件含特殊名称打包再解压后哈希正确。 |
| archive.unzip.selected | CAP-ZIP-003 | ZIP；目标目录、冲突规则 | 解压结果 | DEP-ZIP | M；普通未加密 ZIP | AC-CAP-079：真实解压、部分失败及坏包反馈正确，路径不越界。 |
| archive.zip.contents | CAP-ZIP-002 | ZIP；列表排序/过滤 | 归档内部路径列表 | DEP-ZIP | M；归档结构可读 | AC-CAP-080：目录和文件完整列出，列表不依赖提取全部文件。 |
| archive.zip.compression-ratio | CAP-ZIP-004 | ZIP；总体/条目维度、显示精度 | 原大小、压缩后大小和明确公式的百分比 | DEP-ZIP | M；可获取归档尺寸 | AC-CAP-081：已知字节样本计算正确，零长度分母单独处理。 |
| archive.zip.move-to-screenshots | CAP-ZIP-005 | 选区；参数化目标文件夹、归档名、源保留意图 | 移动后的目录与 ZIP，逐阶段结果 | DEP-FS、DEP-ZIP | M；源/目标权限满足 | AC-CAP-082：正常两阶段成功；压缩失败时准确说明已移动部分且不丢唯一文件。 |

### 3.6 File actions：20 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| metadata.file-size | CAP-FILE-010 | 文件；逻辑大小/占用空间、单位 | 字节数及单位说明 | DEP-FS | M；属性可读 | AC-CAP-083：已知大小及稀疏文件样本区分逻辑与占用字节。 |
| metadata.folder-size | CAP-FILE-011 | 文件夹；递归、链接、隐藏项及单位规则 | 总大小、统计范围和不可访问项 | DEP-FS | M；目录可遍历 | AC-CAP-084：受限子目录不会被静默遗漏为完整总数，链接不重复递归。 |
| system.current-directory | CAP-FILE-012 | 当前显示的 Finder 目录与会话目录状态 | 最新目标、已应用目录和同步状态 | DEP-FS、DEP-PLATFORM、DEP-TERM | M；有效上下文 | AC-CAP-085：同步中不把目标冒充实际 cwd；完成后两者一致。 |
| file.find-largest-files | CAP-FILE-013 | 目录；递归范围、最大条数、大小口径 | 按大小排序的文件结果及路径 | DEP-FS | M；范围可读取 | AC-CAP-086：固定样本前 N 项与排序正确，权限不足显示不完整范围。 |
| file.find-duplicates | CAP-FILE-014 | 目录；递归、大小/内容规则、链接处理 | 按内容分组的重复文件列表 | DEP-FS | M；文件可读 | AC-CAP-087：同内容不同名归一组，同名不同内容不误判，不自动删除。 |
| file.trash.selected | CAP-FILE-008 | 单项/多项选区 | 回收站结果与失败项 | DEP-FS、DEP-PLATFORM | M；回收站适用 | AC-CAP-088：多项确实可恢复，不能只处理第一项或改为永久删除。 |
| organization.tidy-folder | CAP-FILE-007 | 当前目录；分类目标/规则、递归边界 | 可核对的整理计划与实际移动结果 | DEP-FS、DEP-MODEL | M；权限满足，规则形成可执行计划 | AC-CAP-089：按类型和用户自然语言分类各完成一次，范围外文件不改。 |
| file.move.subfolder | CAP-FILE-004 | 选区；子文件夹名称、冲突规则 | 新/已有子目录与移动结果 | DEP-FS | M；当前目录可写 | AC-CAP-090：参数化目录名、多选移动完整，目标不能落入源自身形成循环。 |
| metadata.file-type | CAP-FILE-009 | 文件；内容探测及扩展名对比 | 真实类型/无法确定的说明 | DEP-FS | M；文件可读 | AC-CAP-091：文本、二进制、伪扩展名都不只按文件名判类型。 |
| metadata.download-source | CAP-FILE-015 | 文件；来源与下载时间字段 | 系统保存的来源 URL/时间或未记录 | DEP-PLATFORM | M；系统元数据存在/可读 | AC-CAP-092：有来源字段准确呈现，无字段不猜测网站。 |
| file.rename.add-date-suffix | CAP-FILE-005 | 文件/文件夹；日期来源、格式、时区、后缀位置 | 带日期的新名称 | DEP-FS | M；父目录可写 | AC-CAP-093：无扩展名、多扩展名和不同时区参数按预览改名。 |
| file.rename.add-number-suffix | CAP-FILE-006 | 多文件；排序、起始、步长、位数 | 带编号的唯一名称 | DEP-FS | M；父目录可写 | AC-CAP-094：至少两种排序和编号宽度结果可预测且不覆盖同名文件。 |
| file.rename.add-new-suffix | CAP-FILE-005 | 文件；用户后缀文本、插入位置 | 参数化后缀名称 | DEP-FS | M；父目录可写 | AC-CAP-095：后缀由用户参数决定，扩展名按规则保留。 |
| file.rename.lowercase | CAP-FILE-005 | 多文件；大小写转换范围、扩展名处理 | 小写名称与冲突报告 | DEP-FS | M；文件系统命名规则允许 | AC-CAP-096：大小写不敏感文件系统碰撞被检测，不能静默覆盖。 |
| organization.by-date | CAP-FILE-007 | 目录/选区；日期字段、时区、目录格式 | 日期分类目录与整理结果 | DEP-FS | M；日期元数据和权限可用 | AC-CAP-097：创建/修改/拍摄日期的选择明确，缺字段不隐式改用别的日期。 |
| metadata.finder-greyed-out | CAP-FILE-016 | 文件；Finder 状态、权限/标记等属性 | 基于可读证据的原因与建议 | DEP-PLATFORM、DEP-FS、DEP-MODEL | M；必要属性可访问 | AC-CAP-098：已知隐藏/权限/云状态样本引用真实属性，不确定原因明确标注。 |
| finder.select-mp4 | CAP-FILE-017 | 当前目录；参数化扩展名、是否包含隐藏项 | 匹配文件并在 Finder 选择 | DEP-FS、DEP-PLATFORM | M；Finder 控制权限 | AC-CAP-099：只选当前层匹配项，大小写后缀规则明确，不扩展到子目录。 |
| finder.find-mp4-deep | CAP-FILE-018 | 当前目录；扩展名、递归范围 | 深层匹配列表与定位/选择动作 | DEP-FS、DEP-PLATFORM | M；目录和 Finder 权限满足 | AC-CAP-100：多层文件完整列出，跨目录结果分组定位，不遗漏不可同时选择项。 |
| finder.select-pdfs-containing-apple | CAP-FILE-019 | 当前目录；用户关键词、匹配规则、递归范围 | 正文命中的 PDF 列表及 Finder 选择/定位 | DEP-PDF-CONTENT、DEP-OCR、DEP-FS、DEP-PLATFORM | M；正文可读，扫描页按需 OCR | AC-CAP-101：用户参数关键词匹配正确；未能检索的加密/坏文件单列说明。 |
| finder.find-pdfs-containing-apple | CAP-FILE-020 | 用户指定本机搜索范围；关键词、索引/全文策略 | 全部可访问范围的 PDF 命中结果与位置 | DEP-PDF-CONTENT、DEP-OCR、DEP-FS、DEP-PLATFORM | M；范围权限与必要索引/内容可用 | AC-CAP-102：跨目录命中可定位；系统受限/未索引部分明确显示，不把局部结果称全盘完整。 |

### 3.7 Calculations：4 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| calculation.percent.15-of-85-99 | CAP-CALC-001 | 百分比、基数、精度、取整方式 | 计算结果及单位/公式含义 | DEP-MATH | M；数值有效 | AC-CAP-103：多个用户参数与独立高精度结果一致，不使用来源键里的数字。 |
| calculation.height.feet-inches-to-cm | CAP-CALC-002 | 英尺、英寸、目标单位、精度 | 身高/长度换算值 | DEP-MATH | M；单位与范围有效 | AC-CAP-104：混合英制单位转换准确，无效/歧义单位先澄清。 |
| calculation.time.day-to-seconds | CAP-CALC-003 | 数量、源时间单位、目标单位 | 时间长度换算 | DEP-MATH | M；固定时长单位有效 | AC-CAP-105：天/小时/秒互转正确，日历日期与固定时长不混同。 |
| calculation.sqrt.1764 | CAP-CALC-004 | 数值、精度与实数/复数范围 | 平方根结果或输入范围反馈 | DEP-MATH | M；计算范围有效 | AC-CAP-106：整数/小数计算正确，负数按明确范围处理，不虚构实数答案。 |

### 3.8 System & Utilities：13 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| system.open-terminal | CAP-SYSTEM-002 | 当前目录；系统终端应用或 Fleqi 终端目标 | 在目标目录打开可交互终端 | DEP-PLATFORM、DEP-TERM | M；应用可用、有效目录 | AC-CAP-107：目标终端实际 cwd 正确，含特殊字符路径安全到达。 |
| system.eject-current-volume | CAP-SYSTEM-003 | 当前目录对应或显式选择的可移除卷 | 弹出状态及忙碌/不可弹出原因 | DEP-PLATFORM | M；可弹出卷存在、权限满足 | AC-CAP-108：测试卷真正卸载；系统卷和占用卷不能被误报成功。 |
| system.caffeinate.hour | CAP-SYSTEM-004 | 用户指定持续时间、保持唤醒类型 | 有期限的系统电源断言与剩余状态 | DEP-PLATFORM | M；电源能力可用 | AC-CAP-109：期限内断言存在，到期/取消后释放，不永久遗留。 |
| system.toggle-dark-mode | CAP-SYSTEM-005 | 切换或指定明/暗系统外观 | 系统外观状态 | DEP-PLATFORM | M；系统设置授权满足 | AC-CAP-110：系统真实外观变化，权限失败不只改 Fleqi 局部主题。 |
| system.finder.show-hidden-files | CAP-SYSTEM-006 | 显示/隐藏 Finder 隐藏项 | Finder 实际显示状态 | DEP-PLATFORM | M；Finder/系统设置权限 | AC-CAP-111：测试隐藏文件在 Finder 显隐符合目标，必要刷新及影响可见。 |
| system.print | CAP-SYSTEM-007 | 文件；打印机、份数、页范围及参数 | 打印队列任务 ID/受理或失败 | DEP-PLATFORM | M；可用打印机与格式支持 | AC-CAP-112：测试队列真实收到正确参数；仅受理不能宣称纸张已打印完成。 |
| system.sleep-now | CAP-SYSTEM-008 | 立即/明确延迟的休眠请求 | 休眠请求及恢复后的记录 | DEP-PLATFORM | M；系统允许休眠 | AC-CAP-113：受控设备实际进入休眠，执行前记录持久化，恢复后状态可核对。 |
| system.processor | CAP-SYSTEM-009 | 当前设备；显示字段 | CPU/SoC 名称、架构与可获得信息 | DEP-PLATFORM | M；系统信息可读取 | AC-CAP-114：Apple Silicon 与 Intel 适配字段正确，不假设固定旧查询字段。 |
| system.ram | CAP-SYSTEM-010 | 当前设备；单位、总量/使用量范围 | 内存信息及口径 | DEP-PLATFORM | M；系统信息可读取 | AC-CAP-115：总容量与系统 API 一致，使用量与总量不混淆。 |
| system.monitor-resolution | CAP-SYSTEM-011 | 目标显示器或全部；物理/逻辑口径 | 每个显示器分辨率及缩放关系 | DEP-PLATFORM | M；显示器信息可读取 | AC-CAP-116：多显示器/缩放场景逐项正确，不把逻辑点数当物理像素。 |
| system.charger-wattage | CAP-SYSTEM-012 | 当前设备；适配器额定功率或实际可测口径 | 系统可读的充电功率字段与含义 | DEP-PLATFORM | M；硬件/系统提供字段 | AC-CAP-117：标明额定与实时区别；未提供字段或未接电源时不给虚构瓦数。 |
| system.battery-charge-time | CAP-SYSTEM-013 | 当前设备；电源/电池状态 | 系统估计充满时间与状态 | DEP-PLATFORM | M；有电池及可用估计 | AC-CAP-118：充电/已满/不充电/无估计分别说明，不把未知当零分钟。 |
| system.hide-file | CAP-SYSTEM-014 | 文件列表；隐藏/取消隐藏目标 | 平台隐藏属性与 Finder 可见性结果 | DEP-PLATFORM、DEP-FS | M；属性修改权限 | AC-CAP-119：属性确实改变，显示隐藏文件模式下的可见性区别明确。 |

### 3.9 Developer：12 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| shell.pwd | CAP-FILE-012 | 当前 Session；查询实际 cwd | PTY 实际目录及 Finder 目标差异 | DEP-TERM | M；Session 已建立 | AC-CAP-120：原始终端 cwd 查询与已确认目录一致；忙碌时不把查询输入打入交互程序。 |
| git.is-repo | CAP-DEV-001 | 当前/指定目录 | 是否 Git 仓库、仓库根或错误 | DEP-GIT | M；目录可读 | AC-CAP-121：普通目录、仓库子目录、工作树均判断正确，错误非简单伪否。 |
| git.pull | CAP-DEV-002 | 仓库；远端、分支、合并/变基策略 | 拉取结果、提交变化或冲突 | DEP-GIT | M；仓库、网络、远端认证 | AC-CAP-122：测试远端真实拉取；冲突保留并展示，不自动重置用户改动。 |
| git.switch-dev | CAP-DEV-003 | 仓库；目标分支、是否明确允许创建 | 切换结果与当前分支 | DEP-GIT | M；分支存在或明确创建意图 | AC-CAP-123：分支参数化，未提交改动冲突按 Git 结果说明，不强制丢弃。 |
| git.commit-and-push | CAP-DEV-004 | 仓库；暂存范围、提交消息、远端、分支 | 暂存/提交/推送的逐阶段结果 | DEP-GIT | M；身份、远端认证与网络 | AC-CAP-124：明确“全部”时包含全部请求范围；推送失败保留已提交事实，不默认强推。 |
| code.count.javascript-lines | CAP-DEV-005 | 目录；扩展名、排除/递归规则、行数口径 | 文件数量、逐项/总行数 | DEP-FS | M；范围可读 | AC-CAP-125：JS 默认意图及用户改扩展名均正确，依赖目录排除规则公开。 |
| developer.file-checksum-sha256 | CAP-DEV-006 | 文件；算法与输出格式 | SHA-256 摘要及文件对应关系 | DEP-FS | M；完整文件可读 | AC-CAP-126：固定向量与独立 SHA-256 一致，大文件不因截断给错摘要。 |
| developer.remove-quarantine | CAP-DEV-007 | 文件列表；隔离属性移除意图 | 实际属性变化及逐项失败 | DEP-PLATFORM | M；属性存在/可修改 | AC-CAP-127：只移除指定隔离属性，缺属性如实说明，不递归扩大范围。 |
| tools.which-brew | CAP-TOOLS-001 | 系统包管理器标识/当前工具环境 | 是否安装、路径、版本和识别来源 | DEP-TOOLS | M；系统环境可查询 | AC-CAP-128：Homebrew 已装/未装/路径异常均正确；检测本身不自动安装。 |
| tools.brew-list-installed | CAP-TOOLS-002 | 包管理器；全部/用户请求/依赖列表口径 | 已安装包列表与版本/来源 | DEP-TOOLS | M；指定包管理器可用 | AC-CAP-129：Homebrew 清单与真实环境一致，叶子包和全部包范围明确。 |
| tools.brew-install-ghostscript | CAP-TOOLS-003 | 用户指定包名、来源、版本 | 安装进度、完成检测与来源记录 | DEP-TOOLS | M；安装来源、网络及权限满足 | AC-CAP-130：首版能安装 Ghostscript 场景及其它目录包；遵守当前 AI 策略与完整性校验。 |
| tools.brew-uninstall-ghostscript | CAP-TOOLS-004 | 指定已安装包、来源、卸载范围 | 卸载结果及真实剩余状态 | DEP-TOOLS | M；目标由指定管理器管理、权限满足 | AC-CAP-131：仅卸载指定包；共享依赖/用户数据不随意删除，状态重新检测。 |

### 3.10 Other：4 项

| legacy_id | 新能力 | 参数与输入 | 输出 | 依赖 | 平台/条件 | 独立验收 |
|---|---|---|---|---|---|---|
| system.ping-google | CAP-NETWORK-001 | 用户目标主机、次数、超时、探测协议 | 延迟、丢包及协议/失败说明 | DEP-NET、DEP-PLATFORM | M；网络及协议权限可用 | AC-CAP-132：目标参数化；可达/不可达样本统计正确，协议替代需明示。 |
| web.download-url | CAP-NETWORK-002 | 用户 HTTP(S) URL、保存路径、重名策略 | 下载文件、进度、来源与真实状态 | DEP-NET、DEP-FS | M；网络、目标权限及必要站点认证 | AC-CAP-133：真实字节与服务样本一致；重定向/404/中断不留下伪成功文件。 |
| weather.tokyo | CAP-NETWORK-003 | 用户地点、单位、当前/指定时间范围 | 天气信息、地点解析、来源和观测/预报时间 | DEP-NET、DEP-WEATHER | M；网络与天气源可用 | AC-CAP-134：不同地点参数返回对应来源结果；歧义地点先澄清，服务失败不编天气。 |
| messages.send-example | CAP-SYSTEM-015 | 用户收件人/服务、文本或选中文件、发送内容 | 服务受理/发送失败与目标说明 | DEP-MESSAGES、DEP-FS | M；消息账号登录、收件对象有效、系统授权 | AC-CAP-135：受控测试收件人真实收到文本/附件；未登录/错误收件人明确失败，不代填历史示例。 |

## 4. 映射边界与实施约束

1. CAP-FILE-012 同时承担 Finder/PTY 目录说明与安全的实际 cwd 查询。目录未同步时显示两者差异；不能为了查询把命令注入正在运行的交互程序。
2. 同一改名、缩放、旋转或提页能力的不同来源场景是参数化验收，不强制为每个固定数字复制业务实现。
3. 图像扩展格式和富文本文档格式由依赖目录提供可工作的解码/转换路径。逻辑依赖只是解耦名称，不能以“接口已存在”替代首版真实实现。
4. 无重编码裁剪与精确裁剪分别验证；PDF 结构压缩不能因历史实现使用过降质工具就默认降质。若请求涉及额外质量变化，形成明确参数和计划。
5. 复制/移动后压缩、Git 提交后推送等组合动作逐阶段记录。不能因为最后一步失败隐瞒前面已经完成的副作用，也不能未经用户意图自行撤销。
6. 文件/图片/PDF 摘要和系统状态解释以真实读取结果为依据。模型不可用时保留原始结果；需要模型的摘要路径明确失败，不伪装完成。
7. 所有实际外发能力都使用用户给定目标；消息/Git/下载/天气条件分别验收。AI 策略与手动终端的区别不因操作属于这些分类而改变。
8. 硬件条件能力在具备设备的验收环境完成成功路径，并在不具备设备的环境完成状态反馈路径。两类结果都进入发布证据。
9. 所有正式版必须具有真实文件处理能力。Windows/Linux 后续复用能力意图，按平台提供系统等价行为或明确平台说明；macOS 首版全部操作意图不因此推迟。

## 5. 发布核对表

| 核对项 | 必须满足 |
|---|---|
| 基础能力 | 六类，共 30 行；AC-CAP-001 至 AC-CAP-030 连续且唯一。 |
| 历史来源 | 十类，分别为 10、18、7、12、5、20、4、13、12、4 行，总计 105 个唯一 legacy_id。 |
| 来源验收 | AC-CAP-031 至 AC-CAP-135 连续且唯一；每行都有 CAP、输入参数、输出、依赖和平台/条件。 |
| 执行证据 | 真实 macOS App 执行结果；界面截图和浏览器测试不能代替文件、PTY、系统、网络或账号能力证据。 |
| 条件覆盖 | 依赖缺失/安装、权限不足、账号未登录、无设备、网络失败与正常条件均有对应结果。 |
| 旧实现清理 | 不复制旧 shell 模板；固定示例已参数化；原始品牌文字仅允许留在来源键中。 |
| 关联要求 | 每项遵守公共 AC、两种 AI 策略、Session 生命周期和 Finder 目录同步合同。 |
