# Claude SDK bridge
> Rust host 与外部 SDK 之间的私有双工协议。

## 地位
脚本由 Rust 编译内嵌并部署；不是项目脚本或 npm 安装器。

## 逻辑
stdin 首帧必须为 `{"type":"initialize","version":1,"config":{...}}`，随后处理 permission/cancel/shutdown。初始化完成才解析 SDK 并启动 query。

## 约束
- 禁止从 argv 接受配置、密钥或任务正文。
- 错误不得反射原始配置；首帧缺失/非法必须失败，不以空配置执行。
- SDK adapter 保留显式与 ambient 解析兼容；Hub 产品必须传入经过只读安装校验的绝对入口，不使用 ambient 路径。

## 业务域清单
| 文件 | 职责 |
|---|---|
| index.mjs | 初始化协议、SDK 解析、query 与权限响应 |
