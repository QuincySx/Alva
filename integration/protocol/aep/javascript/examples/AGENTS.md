# JavaScript plugin example bundle
> 用户显式复制后检查/批准的echo插件。
## 地位
作者示例，测试通过相同Go loader及Hub产品装配它。
## 逻辑
echo.mjs从bundle内alva-sdk.mjs导入；README指示从固定源码复制SDK，manifest请求log/metadata。
## 约束
不隐式启动、不下载SDK、不读取ambient配置；仅stdout协议，Node仍是原生宿主代码。
## 业务域清单
| 文件 | 职责 |
|---|---|
| echo.mjs | echo Tool与当前事件metadata/log，真实异步host调用 |
| alva.toml | 显式identity/runtime/entry和最小回调请求 |
