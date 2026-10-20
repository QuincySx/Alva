# WASIp1 plugin execution peer
> 独立模块的受限stdio执行owner；不拥有插件协议或产品装配。
## 地位
Native wazero叶子，仅标准库/wazero，不依赖AEP、SDK、产品或存储。
## 逻辑
Start同步编译校验，fresh runtime/instance、受限memory和生命周期、总stdio预算；只实例化WASIp1，不给preopen/env/network/宿主imports。外层AEP复用Input/Output/Errors及实际退出/Close接口。
## 约束
无配置扫描/凭据/宿主文件grant。context取消关闭stdio使阻塞WASI调用退出；done在runtime、实例和producer pipe真实收尾后发布，natural exit保留consumer至EOF drain，owner Close关闭剩余端点。CleanupComplete仅在owner关闭且done后成立。Start失败自己释放部分资源；返回后由caller owner关闭。独立硬期限从编译起计；caller context只约束准入。错误不重启或重放。实际WASM运行与交叉编译分别验收。
## 业务域清单
| 文件 | 职责 |
|---|---|
| peer.go | 显式Config、Start、bounded stdio与物理生命周期 |
| peer_test.go | 实际WASM内存增长/IO总量/CPU期限、阻塞pipe取消和自然EOF回归 |
