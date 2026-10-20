# Storage contract acceptance helpers
> 仅按契约装饰调用方提供的存储，不暴露或发现原生实现。
## 地位
测试支撑；生产代码不可依赖。直接消费共享storage/SDK契约，不导入SQLite/native适配。
## 逻辑
Provider包裹存储、会话和引用返回值；actual SameAuthority遵守同一provider和底层真实权威。
记录方法调用次数，让装配测试确认同一插入组件贯穿产品、harness、工具和引用路径。
## 约束
不按名称/ID伪造权威，不另开数据库或实现第二后端，不记录正文/凭据，不供生产使用。
## 业务域清单
| 文件 | 职责 |
|---|---|
| provider.go | Opaque storage/session/reference decorators and safe call observations |
