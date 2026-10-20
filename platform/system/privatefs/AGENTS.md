# 横向支撑 · Private filesystem primitives
> 私有目录与文件原语；不拥有SQLite、会话或产品授权。
## 地位
system底层能力；storage、进程适配、插件和产品显式消费，SDK不依赖。
EnsureDir/OpenFile两项在各平台提供同名入口；额外DACL检查/只读handle属于Windows适配。
## 逻辑
Unix复用原有0700/0600、UID、single-link和SameFile检查；最终symlink拒绝，合法祖先alias解析。
Windows新建时附带protected current-user-only DACL；按handle校验owner/ACE/reparse/hardlink。
原始路径拼写先于Windows Abs检查，拒绝被规范化抹掉的尾随点/空格、设备/UNC及drive-relative别名。
子文件继承DACL不等于继承owner；普通OS创建的journal可采用token默认owner，严格CheckFile仍只接受current-user owner。
不支持的ownership平台明确失败。数据库锁继续归storage/sqlite，不随目录操作进入消费者。
## 约束
不修复或收紧既有宽权限对象，不读取配置/账号。Windows只接受普通本地drive路径；实机验收独立。
本次是既有Unix路径逻辑归回现有privatefs，不引入第二文件系统/存储抽象或兼容alias。
## 业务域清单
| 文件 | 职责 |
|---|---|
| paths_other.go | 非Windows EnsureDir/OpenFile及现有mode/SameFile校验 |
| owner_unix.go / owner_other.go | 实际Unix UID/hardlink；未支持平台拒绝 |
| paths_unix_test.go | 无SQL依赖的私有路径、宽权限不变、symlink/hardlink验收 |
| private_windows.go | 原子DACL创建与真实handle身份、CheckDir/File和OpenRead |
| private_windows_test.go | Windows ACE/继承/宽权限不变/hardlink/ADS/reparse验收 |
