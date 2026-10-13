# Changelog

## 0.3.0-beta.5 - 2026-10-11

- 本地测试包，暂未上传或发布；继续使用开源版的独立配置目录与 GitHub 更新来源。
- 兼容整合 PR #1、#3、#4：新增两种字体、Mac 菜单栏模式隐藏程序坞图标、并发用量读取共享一次采集；当前只在本地测试，不合并远端 PR。见 [PR 整合记录](docs/PR-INTEGRATION-20261012.md)。
- 修复 Token 有、金额没有的新用户问题：内置 GPT-5.6、GPT-6、GPT-6.1 Sol 参考价格，保留自己的和共享空间的价格。缺价与缺历史计价信息分别解释，未知金额不填零。
- 首次安装自动统计并缓存本机用量；找不到 Codex 目录时可选择并确认读取，不再把缺少记录显示成零用量。
- 黑底、白底图标重新修整，运行中的图标跟随主题切换；安装包默认使用黑底图标。
- 两版图标都使用圆角底板和透明四角；白天图标恢复原版淡紫色泡泡，黑暗图标保留浅蓝泡泡。
- 原图整体稍微缩小，给泡泡、圆环与圆角底板之间增加留白，Mac 与 Windows 图标同步。
- Mac 和 Windows 新安装时默认字号为 150%，保留已有的手动设置，200% 字号重启后也不会被改小。
- 可选择系统默认、Codex 风格系统字体栈、微软雅黑、得意黑、霞鹜文楷屏幕版、Times New Roman 和方正清刻本悦宋；微软雅黑、Times 和方正字体需自行安装，没有安装时使用系统字体。
- 修复 Plus 双圆环数字偏离圆心的问题；没有读到 5 小时额度时显示已知的周额度，不再留下空卡片。
- 可以关闭背景模糊，去掉导致胶囊发糊的过渡副本，并减少 Windows 展开、收起时重复调整窗口的开销。
- 铃铛也能识别“已安排重置”公告，不把公告当成概率；打开提醒后仍可正常收回胶囊。
- 保留用量缓存及数据完整性修复，未知金额与真实零花费仍分别显示。

## 0.3.0-beta.4 - 2026-10-02

- fix: keep release Rust caches restore-only

## 0.3.0-beta.3 - 2026-10-02

- docs: teach Cockpit support skill signed update behavior
- fix: respect update channel changes and bound test workers
- fix: portable signed build config for Mac and Windows
- feat: signed updates from original GitHub release channels
- fix: allow unanswered comfort prompt to collapse
- fix: restrict control center window drag to header
- docs: identify project website as official website
- docs: add optional coffee support section
- docs: link creator's Bilibili video from READMEs
- docs: add Codex install and support skill with repository map
- docs: make README and release notes easier to read
- fix: emit cross-platform checksums and retain scan evidence on reruns
- fix: match release checksums to normalized asset names
- fix: reject skipped Defender scans and verify release artifacts

## 0.3.0-beta.2 - 2026-09-26

- test: make release checks independent of runner timezone

## 0.3.0-beta.1

- Clean source import, retaining upstream/font licenses.
- Configurable Cloudflare endpoint, create/join setup and custom members.
- Task/project-name sharing opt-in; OS-stored, server-scoped credentials.
- Self-hosting Skill, privacy guidance and Mac/Windows release pipeline.
- No author-owned sync service/runtime data; auto-update disabled.
