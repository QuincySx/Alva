# Public consumer examples
> 独立module消费公开接口，不充当产品默认配方。
## 地位
仓库外部接入示例；不进入根module的生产依赖。
## 逻辑
每个示例有自己的go.mod；相对replace只绑定当前checkout，验证真实公开包可用。
## 约束
无凭据、真实模型或隐式配置；不导入internal、tests、harness或平台后端。发布接入须将replace换成明确版本。
## 业务域清单
| 目录 | 职责 |
|---|---|
| sdk-agent/ | 只用SDK的Plugin/Tool/Middleware和可取消默认循环；native/Node/WASIp1共用源码 |
