# 设计参考

`index.html` 是悬浮球的网页交互原型，用浏览器直接打开即可。后续功能的交互和文案以它为准。

| 文件 | 用途 |
|---|---|
| `index.html` | 交互原型 |
| `orb_clean.png`、`capsule_clean.png`、`wallpaper.jpg` | 原型用到的图片 |
| `orb_glass.png`、`capsule_glass.png`、`glass_v2_states.png`、`glass_v2_morph.png` | 玻璃质感的效果图 |
| `hark.css`、`hark-hero.css`、`hark.js` | 原型依赖的 Hark 组件库样式和脚本 |

`hark.css`、`hark-hero.css`、`hark.js` 来自 [Hark](https://hark.com) 的组件库（见各文件开头的说明），保留它们只是为了让原型能打开。应用本身（`src/`、`src-tauri/`）没有用到它们；它们的版权和许可以 Hark 的条款为准，不在本仓库 MIT 许可证的范围内。
