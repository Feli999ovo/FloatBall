# 服务商 logo 的来源和许可

这里的 224 个 `<服务商 id>.svg` 原样取自 [models.dev](https://github.com/sst/models.dev) 仓库 `providers/<服务商 id>/logo.svg`（dev 分支，2026-11-14 取用），文件内容未改动。models.dev 以 MIT 许可发布，许可全文见同目录的 `LICENSE`。

- `lilac.svg`、`tempr.svg`：这两个服务商后来从 models.dev 删掉了，文件取自删除前的版本（提交 `89c02ab5ee` 和 `7e5a4a89d5` 的上一个版本）。
- 核对方法：`curl -s https://raw.githubusercontent.com/sst/models.dev/dev/providers/<id>/logo.svg | md5sum` 与本地文件比对，2026-11-15 逐个核对一致。
- 没有 logo 的服务商不放图，界面上用服务商名字的第一个字代替。
- `index.js` 是本项目生成的 id 列表，不是 models.dev 的文件。

MIT 许可只管这些 SVG 文件本身。图里的商标和标志归各自公司所有，这里只用来在设置里标出模型属于哪个服务商。
