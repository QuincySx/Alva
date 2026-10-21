# L4 · Skills Plugin
> Skill 协议的 prompt/Tool 装配，不自动执行脚本或扩大工具授权。

## 地位

可选 Plugin；依赖协议仓库和绑定会话的引用后端，SDK 不依赖本层。

## 逻辑

注册时只注入 auto 元数据，skill 工具按 exact name 读取正文/资源；大正文先存引用再按需读取。

## 约束

actual session identity 不匹配时拒绝，不给 child 借父目录/缓存；二进制不自动送模型。
Tools() 供受控 harness 获取同一实例的安全装配信息，不是通过名字申请授权。
独立SDK调用也必须匹配ActualTool/CallID/name；工具自己校验单个4KiB UTF-8 JSON、非null字段和相对资源path，不依赖可选schema Plugin兜底。确定性坏参数为IsError反馈，缺失实现/后端失败/取消仍返回error。
Register拒绝typed-nil repository/reference，目录≤1024；读取回调后重查ctx，selected document/resource身份、UTF-8、1MiB正文及有界元数据都需有效。引用必须Validate成功，不将取消、坏引用或JSON替换非法字节冒充成功。

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Plugin | `plugin.go` | 小目录 prompt、skill Tool 与引用优先装配；显式正文结果保留有界 compatibility 要求，声明不授予权限 |
| Regression | `plugin_test.go` | 无正文预载、显式加载、引用与跨会话拒绝 |
| Repository boundaries | `boundary_test.go` | 替换仓库的参数帧/实际调用/typed-nil/迟到取消/错误来源与引用边界 |
