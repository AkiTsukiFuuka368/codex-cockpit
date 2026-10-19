# Claude 支持

这个分支在驾驶舱里加了 Claude：

- **额度卡片**：显示 Claude 订阅的 5 小时窗口和每周窗口剩余百分比、重置时间。
- **控制中心 → 用量页**：Codex 和 Claude 左右并列，Claude 一栏与 Codex 功能相同（分组、时间段、按任务/模型、日历、每日趋势、Token/成本切换）。任务即 Claude Code 会话，子 Agent 归入所属会话。金额按每次请求的 Token 乘官方 API 单价自动计算。Claude 栏下方有额度采集设置和官方价格表。窗口较窄时两栏上下排列。

## 数据从哪里来

**Token 用量**：读取 Claude Code 在本机写下的会话记录 `~/.claude/projects/**/*.jsonl`（设置了 `CLAUDE_CONFIG_DIR` 时读那个目录）。读取用量数字、模型名、时间和项目文件夹名；任务标题取会话标题，没有时取第一条提问的开头，只在本机显示，不保存、不上传。网页版和 App 里的普通聊天不在本机留记录，统计不到。

**订阅额度**：不使用你的登录凭证去请求任何接口。Claude Code 自己会把 5 小时和每周额度交给状态栏脚本（`rate_limits` 字段）。在 Claude 页点「一键配置」后：

1. 写入 `~/.claude/cockpit/statusline.ps1`，并把 `~/.claude/settings.json` 的 `statusLine` 指向它（原文件先备份为 `settings.json.cockpit-backup`；如果你已有自己的状态栏，不会改动，除非你选择替换）。
2. 脚本把额度写到 `~/.claude/cockpit/rate-limits.json`，驾驶舱只读这个文件。
3. 同时写入 `~/.claude/cockpit/claude-run.ps1`：给 Codex 等程序用来代替 `claude -p`。它会找到 claude.exe（包括 Claude 桌面端自带的那份）、读取用户环境变量里的 `CLAUDE_CODE_OAUTH_TOKEN`、输出和 `claude -p` 一样的结果，并记下运行中收到的额度事件。

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File "$HOME\.claude\cockpit\claude-run.ps1" "任务内容"
Get-Content task.md -Raw | powershell -NoProfile -ExecutionPolicy Bypass -File "$HOME\.claude\cockpit\claude-run.ps1" --stdin
```

额度只在 Claude Code 发出请求时更新。卡片上的时间是最近一次采集时间；窗口过了重置时间会自动显示为 100%。

## 金额说明

价格按 Anthropic 官方 API 价格页（2026-10-18 核对）内置，包含缓存写入（5 分钟 / 1 小时）和缓存读取单价，Haiku 5.5 按单次请求是否超过 10 万 Token 区分档位。订阅用户不会按这个扣费，只作参考。价格表里没有的模型不计入金额。

## 自动更新

这个分支的安装包会从本仓库自己的更新源（`updates-stable` / `updates-beta` 发布中的 `latest.json`）检查更新，不会连到原作者的仓库。每次推送到 `claude-support` 分支，GitHub Actions 会编译、签名并刷新更新源，已安装的驾驶舱会在「设置 → 应用更新」里自动下载新版，确认后重启安装。签名需要仓库里有 `TAURI_SIGNING_PRIVATE_KEY` 这个 Actions 密钥；没有时仍会出安装包，但不能自动更新。

## Codex 计价补充

- `codex-auto-review`（自动审查子任务）按 $0 计：OpenAI 说明自动审查免费、不计入套餐用量，也没有公开的 API 单价。需要的话可在自定义价格表里覆盖。
- 续接或分叉的会话里，一次性累加的用量无法按单次请求判断是否超过 272K 长上下文档，现在按标准档估算（增量不超过 272K 时与精确值相同，超过时是下限），不再显示为“成本不全”。
