# 安全问题报告

如发现可能影响悬浮球或用户数据的安全问题，请使用 GitHub 仓库的 **Private vulnerability reporting（私密漏洞报告）** 提交报告。请勿在公开 Issue 中披露尚未修复的问题。[提交私密漏洞报告](https://github.com/Feli999ovo/FloatBall/security/advisories/new)

悬浮球没有自己的服务器。API Key 存在操作系统钥匙串（macOS 钥匙串 / Windows 凭据管理器），而不是应用配置文件中。使用模型、搜索或本机终端工具时，数据可能按你的配置发送给相应服务；报告时请注明相关组件和复现步骤，并避免附上真实密钥或个人数据。
