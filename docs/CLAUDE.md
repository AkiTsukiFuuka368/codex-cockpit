# Claude 支持

这个分支在驾驶舱里加了 Claude：

- **额度卡片**：显示 Claude 订阅的 5 小时窗口和每周窗口剩余百分比、重置时间。
- **控制中心 → 用量页**：Codex 和 Claude 左右并列，Claude 一栏与 Codex 功能相同（分组、时间段、按任务/模型、日历、每日趋势、Token/成本切换）。任务即 Claude Code 会话，子 Agent 归入所属会话。金额按每次请求的 Token 乘官方 API 单价自动计算。Claude 栏下方有额度采集设置和官方价格表。窗口较窄时两栏上下排列。

## 数据从哪里来

**Token 用量**：读取 Claude Code 在本机写下的会话记录 `~/.claude/projects/**/*.jsonl`（设置了 `CLAUDE_CONFIG_DIR` 时读那个目录），以及 Claude 桌面端数据目录（`%APPDATA%\Claude`，商店版在 `Packages\Claude_*\LocalCache\Roaming\Claude`）里找到的其他 `.claude\projects`（桌面端 Code、Cowork 会话）。同一条消息出现在多个目录时只计一次。读取用量数字、模型名、时间和项目文件夹名；任务标题取会话标题，没有时取第一条提问的开头，只在本机显示，不保存、不上传。以下用量不在本机留记录，统计不到，但会消耗同一份订阅额度：claude.ai 网页和手机 App 的聊天；在云端运行的 Claude 会话（claude.ai 里让 Claude 动手做的任务、定时任务、网页版 Claude Code）；其他电脑。Claude 栏下方「统计范围」列出读取的目录，并把最近一次额度读数和同一窗口内的本机用量放在一起，方便看差多少。

**订阅额度**：不使用你的登录凭证去请求任何接口。Claude Code 自己会把 5 小时和每周额度交给状态栏脚本（`rate_limits` 字段）。在 Claude 页点「一键配置」后：

1. 写入 `~/.claude/cockpit/statusline.ps1`，并把 `~/.claude/settings.json` 的 `statusLine` 指向它（原文件先备份为 `settings.json.cockpit-backup`；如果你已有自己的状态栏，不会改动，除非你选择替换）。
2. 脚本把额度写到 `~/.claude/cockpit/rate-limits.json`，驾驶舱只读这个文件。
3. 同时写入 `~/.claude/cockpit/claude-run.ps1`：给 Codex 等程序用来代替 `claude -p`。它会找到 claude.exe（包括 Claude 桌面端自带的那份）、读取用户环境变量里的 `CLAUDE_CODE_OAUTH_TOKEN`、输出和 `claude -p` 一样的结果，并记下运行中收到的额度事件。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$HOME\.claude\cockpit\claude-run.ps1" "任务内容"
Get-Content task.md -Raw | powershell -NoProfile -ExecutionPolicy Bypass -File "$HOME\.claude\cockpit\claude-run.ps1" --stdin
```

额度百分比只在交互式 Claude Code 发出请求时更新（状态栏脚本拿到 `rate_limits`）。`claude -p` 的 `rate_limit_event` 在正常（`allowed`）状态下不带 `utilization`，只有警告（`allowed_warning`）或被限流（`rejected`）时才带，所以只用 claude-run.ps1 时卡片基本不会更新。卡片上的时间是最近一次采集时间；窗口过了重置时间会自动显示为 100%。

## 金额说明

价格按 Anthropic 官方 API 价格页（2026-10-18 核对）内置，包含缓存写入（5 分钟 / 1 小时）和缓存读取单价，Haiku 5.5 按单次请求是否超过 10 万 Token 区分档位。订阅用户不会按这个扣费，只作参考。价格表里没有的模型不计入金额。

## 自动更新

这个分支的安装包会从本仓库自己的更新源（`updates-stable` / `updates-beta` 发布中的 `latest.json`）检查更新，不会连到原作者的仓库。每次推送到 `claude-support` 分支，GitHub Actions 会编译、签名并刷新更新源，已安装的驾驶舱会在「设置 → 应用更新」里自动下载新版，确认后重启安装。签名需要仓库里有 `TAURI_SIGNING_PRIVATE_KEY` 这个 Actions 密钥；没有时仍会出安装包，但不能自动更新。

## Codex 计价补充

- `codex-auto-review`（自动审查子任务）按 $0 计：OpenAI 说明自动审查免费、不计入套餐用量，也没有公开的 API 单价。需要的话可在自定义价格表里覆盖。
- 续接或分叉的会话里，一次性累加的用量无法按单次请求判断是否超过 272K 长上下文档，现在按标准档估算（增量不超过 272K 时与精确值相同，超过时是下限），不再显示为“成本不全”。
- GPT-6.1 Terra（2026-10-26 发布）已加入内置价格：输入 $1.00、缓存读取 $0.05、输出 $5.00 / 百万 Token，超过 272K 的单次请求同样按 2 倍 / 1.5 倍计。官方另列 30 分钟缓存写入 $1.25，Codex 记录里不区分缓存写入，所以不计。

## DeepSeek 对照

Codex 栏和 Claude 栏的用量总额下面多一行：同样的 Token 按 DeepSeek V4.1 Flash（`deepseek-flash`）单价算要多少钱，给出闲时价到工作日高峰价（翻倍）的区间。单价：缓存命中 $0.003、未命中 $0.15、输出 $0.60 / 百万 Token（闲时，2026-09-10 起生效）。Claude 的缓存写入按未命中计。两家分词不同，只看量级。

## 自定义模型价格

「设置 → 自定义模型价格」可以自己填新模型的价格，不用等驾驶舱更新。单位是美元 / 百万 Token：输入、缓存读取、输出，以及只对 Claude 有用的缓存写入（留空按输入价的 1.25 倍 / 2 倍计）。

- 存在本机的 `custom-prices.json`（驾驶舱配置目录），Codex 和 Claude 都用，优先于内置、共享和 Tokei 价格表；不会导出到共享价格表。
- 模型名用记录里的 ID，不分大小写，`openai/`、`anthropic/` 前缀可省略；结尾加 `*` 表示前缀匹配，精确匹配优先，多个前缀取最长的。
- 区块上方会列出「有用量但没有价格」的模型，点一下就加一行。
- 保存后 Codex 会按新价格重新扫描本机记录，Claude 在下次刷新时重算。

## 检查更新

「设置 → 应用更新」里点「检查更新」不再关掉控制中心：当前版本、检查结果、下载进度和「重启并安装」都直接显示在这一栏。从悬浮条上的更新按钮进入时，仍是原来的更新面板。

