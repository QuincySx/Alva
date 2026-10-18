# 横向支撑 · Windows private filesystem
> Windows 原生 DACL 与文件身份原语；不拥有 SQLite、会话或产品授权政策。

## 地位
仅 Windows 实现，由 storage/browser/插件 snapshot 的平台适配消费，不进入 SDK。
## 逻辑
新建目录/文件在创建 syscall 中附带 protected、current-user-only DACL；现有对象按 handle 校验 owner、有效 ACE、reparse 与 hardlink。目录 ACE 向新文件/子目录继承，SQLite journal 也保持私有。
## 约束
不收紧/修复既有宽权限对象；拒绝 UNC/device/ADS/非持久 ACL volume、reparse 祖先与 hardlink 文件。不以 chmod 冒充 Windows 安全；本机交叉编译不替代 Windows runtime 测试。当前仅接受普通本地 drive 路径。
## 业务域清单
| 文件 | 职责 |
|---|---|
| private_windows.go | 原子创建、DACL/current user/路径/handle身份校验，OpenFile写入owner和OpenRead只读OPEN_EXISTING；后者拒绝共享write/delete且不创建文件 |
| private_windows_test.go | Windows 原生 ACE/继承/宽权限不变/hardlink/ADS/reparse 反例 |
