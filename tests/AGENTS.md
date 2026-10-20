# Repository verification
> 跨层静态闸与真实 SDK 消费者实验。

## 地位
不参与产品运行；包内单元测试继续留在所属源码目录。
## 逻辑
architecture 离线检查源码依赖、公开快照、分形文档和固定转换资产；extensibility 实际构建 SDK 消费者并验证可扩展性边界。
## 约束
生产包不得依赖本目录。实验不等于完整平台或产品验收，不通过放宽内核权限或改公开契约让实验通过。
## 业务域清单
| 路径 | 职责 |
|---|---|
| architecture/ | 静态依赖、布局、分形、公开面及来源闸 |
| extensibility/ | 上下文、多 Agent、双向 transport、独立进程恢复与跨目标普通工具CLI消费者 |
| storagecontract/ | 只用共享契约的opaque provider/session/reference测试装饰，验证组件未绕过插口 |
