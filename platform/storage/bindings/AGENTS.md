# 横向支撑 · Runtime identity storage
> 共享BindingStore的原生SQL实现，不拥有外部Agent进程。
## 地位
原有engine.sqlite/native_bindings实现的独立归属；由上层显式LocalBindings选择。
integration/engine/process只接收storage.BindingStore，不导入本包。
## 逻辑
Read按真实native ID读取owner/canonical workspace；Put在一个事务中create-or-verify。
相同绑定幂等，改owner/workspace返回共享ErrBindingConflict且不改变原记录。
沿用原文件名/table/数据；仅源码归属迁移，无历史搬运或新后端。
## 约束
先取得实际进程独占锁；无ambient账号或会话发现。不存在/已关闭用共享错误，SQL仅实现内部可见。
Close成功幂等，失败保留实际资源owner；构造错误不授权启动runtime或选择替代存储。
## 业务域清单
| 文件 | 职责 |
|---|---|
| store.go | 显式打开、私有schema、原子精确绑定和生命周期 |
| store_test.go | 并发单一赢家、owner/cwd冲突、重开和独占/关闭验收 |
