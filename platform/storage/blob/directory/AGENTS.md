# Directory object adapter
> 使用显式目录保存不可变对象，不把云盘当作执行工作区。
## 地位
blob.Store 的 native 文件适配；目录隐私、挂载账号和生命周期由宿主负责。
## 逻辑
Open先clean路径再拒绝末级symlink并pin真实目录，末尾slash不能绕过校验；文件先写临时项并Sync，再hard-link发布随机key；既有值只验证、不覆盖。
Read 拒绝 symlink/非普通文件、越界和过大对象；Close 与活跃操作互斥。
## 约束
文件系统必须支持 hard-link 和 file/directory Sync；失败明确返回，无 direct-write/overwrite fallback。
挂载服务实际持久性不能靠 Go 接口推断；Windows 或不支持这些操作的云盘应实现专用 API adapter。
已有目录权限由显式宿主保证；不调整共享目录权限、不扫描、不清理未知对象或发布后的不确定对象。
取消在 IO 边界检查，不宣称可抢占底层内核文件系统调用。对象 hash 由授权 catalog 验证。
## 业务域清单
| 文件 | 职责 |
|---|---|
| directory.go | pin/open、原子创建或比较、bounded Read 和 owned Close |
