# 横向支撑 · Local HTTP access gate
> 产品共用的严格本地HTTP认证，不承载协议或会话业务。

## 地位
Native transport边界，无Hub/SDK/model依赖；独立Gateway与Hub HTTP共用。
## 逻辑
实际numeric loopback authority，随机256-bit带外Bearer，constant-time比较。
精确Host/Origin/Sec-Fetch-Site，不接受query token或forwarding header作为授权。
Gate返回HTTP状态，调用方保留各自协议error envelope。
## 约束
构造不监听、不读环境、不发现凭据。生产入口先绑定loopback listener，再以真实IP:port构造。
仅验证配置不等于实际wildcard listener已安全。无CORS或反向代理信任。
## 业务域清单
| 文件 | 职责 |
|---|---|
| gate.go | immutable Gate、随机token和shared no-store安全响应头 |
| gate_test.go | authority/token、Origin/重复Bearer/URL token/伪forward拒绝 |
