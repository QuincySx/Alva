# Volatile object adapter
> 进程生命周期的有界内存对象，不接触文件系统。
## 地位
blob.Store 的纯标准库实现；不依赖 SDK、SQLite、其他 adapter 或产品。
## 逻辑
显式字节/对象预算，锁内创建或比较既有值；空对象也计入对象预算。所有读写边界复制 buffer。
## 约束
进程退出即丢失；不能声明 restart-durable。没有自动 eviction 或删除，避免已提交引用悄悄失效。
## 业务域清单
| 文件 | 职责 |
|---|---|
| memory.go | 配置、immutable Put、有界 Read 与容量准入 |
