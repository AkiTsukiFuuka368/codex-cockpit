# 网站 Reset Watch 铃铛

通过 Rust 只读请求读取 [公开 API](https://codex-resets.com/api/resets) 的 `watch`；以 level=elevated/strong、可解析的 observed_at/expires_at 和尚未到期为准，不设概率阈值。当前接口没有投票数据也能显示预警；不把 `reset_chance` 冒充投票百分比，没有可靠数字时概率为 null。第三方预测不是 OpenAI 承诺。

同时识别 `/api/v1/status` 的 `scheduled_reset`：只接受来源、时间和状态通过校验的未来公告，作为“重置已宣布”铃铛显示，不伪造概率。到期、撤回或检查过旧后隐藏。

仅传递信号类型、level、概率、观测/到期时间和本地 watchCheckedAt，不存推文或账户数据。铃铛独立于校准 score 与手动风险滑条，不改变额度或舒适曲线。沿用5分钟原生后台额度事件、启动/聚焦/手动刷新；请求失败或撤回立即清除，下次30秒规划时钟清除到期信号；10分钟没有成功检查也隐藏。不申请系统或浏览器通知。

胶囊徽标不增加点击目标，仍点击展开；展开标题旁按钮阻止拖动冒泡，固定打开 https://codex-resets.com/，不使用上游任意URL。无 Watch 时不占位。

浏览器本地预览 ?watch=active 使用合成70% Watch；页面标明 synthetic，Tauri分支完全忽略该查询参数，始终走真实 API。不能把预览当作实时预警。
