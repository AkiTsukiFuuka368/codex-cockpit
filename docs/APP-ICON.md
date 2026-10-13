# 应用图标

图标使用圆角底板，底板之外是真正透明的四角和留白，不依赖系统替 App 裁圆角。
黑暗版保留黑底、浅蓝玻璃泡泡和蓝紫圆弧；白天版使用原版的白底、淡紫色泡泡与柔和玻璃质感，
不再使用偏蓝的白底重绘版。原图整体缩至画布的 84%，让泡泡和圆环与圆角底板之间多一点空隙；
底板外侧仍保留 6% 的透明留白。图标是固定品牌图案，不代表当前账号的实时用量。

## 源文件与生成

- `src-tauri/app-icon.png`：裁切后的高分辨率黑底图，也是 Tauri 图标生成的输入。
- `src-tauri/app-icon-original-light.png`、`app-icon-original-dark.png`：未裁切原图；白天版从原版 ICNS 的 1024 像素帧恢复，不重画泡泡。
- `src-tauri/app-icon-dark.png`、`app-icon-light.png`：缩放、留白及圆角处理后的主题图标。
- `assets/app-icon.png`：相同的黑底产物，用于应用的浏览器预览和文档。
- `assets/app-icon-dark.png`、`app-icon-light.png`：浏览器预览的主题图标。
- `src-tauri/icons/*.png`、`icon.icns`、`icon.ico`：生成的桌面图标。
- `src-tauri/icons/light/`：配对的白底桌面图标。
- 旧 `icon.svg` 是历史素材，不作为当前应用图标的来源。

在项目根目录运行：

```sh
node scripts/round-app-icons.mjs --write --native
npm test -- scripts/app-icon.test.mjs
```

脚本会在临时目录调用已安装的 Tauri，只把桌面 PNG、ICNS、ICO 写回项目，不改动移动端素材。
Mac 和 Windows 应使用同一组原图，不能只更新其中一个格式。重新生成图标不会自动
替换已经安装的 App；还需要重新打包、安装，系统也可能缓存旧图标。

运行中的 Mac Dock／Windows 窗口图标与网页 favicon 跟随 App 的日间、深色或系统主题。
安装包、Finder／资源管理器和已固定的快捷方式使用静态黑底图标，不承诺自动换色。
托盘实时额度图案保持独立，不被品牌图标覆盖。

## 图像编辑记录

使用内置 imagegen 编辑原来的玻璃图标。最终要求：只将白底改成纯黑；泡泡改为上述
浅蓝色，保留少量淡紫反光；右上圆弧保持淡紫／蓝紫色。保留原有构图、比例、留白
和玻璃质感，黑底透过环之间的间隙，不增加文字、数字、图案或额外背景。

后续修整用 imagegen 保留配色与构图，重新绘制连续平滑的玻璃外沿和紫色弧线，
去掉尖刺、重复高光与粗糙接缝。

圆角修复经用户确认，使用原图做几何 alpha 裁切，不再用 AI 重画。首次修复只改变 alpha；
随后按用户要求增加内部留白，把整张原图等比缩小，使用双线性采样和原背景色填充，不添加新的图案或调色。
背景四角和外侧留白设置透明 alpha，边缘使用抗锯齿覆盖率。白底版使用原版淡紫色图案，
黑底版保留已有蓝色图案。运行 `node scripts/round-app-icons.mjs --check` 可核对产物与原图缩放及留白规则一致。
PNG、ICNS、ICO 同时生成；回归检查源文件与运行时图标的透明角、透明留白、不透明中心与抗锯齿边缘，
避免把“有 alpha 通道但所有像素仍然不透明”的方图误认为已经修复。
