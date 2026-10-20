# JavaScript SDK runtime tests
> Node内存双向流上的真实SDK回归。
## 地位
作者客户端测试，不参与Go生产路径。
## 逻辑
执行真实run/definePlugin，模拟host帧验证反向调用、过期handle、tool错误与取消。
## 约束
不使用凭据或外部服务；不能代替Go实际子进程和WASM运行验收。
## 业务域清单
| 文件 | 职责 |
|---|---|
| runtime.test.mjs | Node内置test的协议、预算、cancel/join与重复ID回归 |
