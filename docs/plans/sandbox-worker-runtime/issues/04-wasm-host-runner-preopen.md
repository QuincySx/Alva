# 04 — wasm 宿主 runner + preopen 圈禁（缝 2 落地）

**What to build:** 一个宿主侧 runner：接收 job 配置（授权目录列表）与 wasm 模块，用 wasmtime 启动沙箱，把授权目录以 preopen 方式映射进 guest。guest 内文件 CRUD 全套可用且被圈禁——授权外路径在 guest 世界不存在。这张票同时开出本项目唯一的新测试缝：runner 公共边界（喂配置+fixture wasm，只断言文件系统效果与返回值）。文件后端保持在 wasi 文件系统接口之后，本地目录只是一种实现。

**Blocked by:** 03 — wasm 跑道预备。

**Status:** ready-for-agent

- [ ] runner 接口：授权目录 + wasm 模块 + 任务入参 → 执行结果，全程无全局状态
- [ ] fixture wasm 经 runner 完成创建/读取/覆写/删除/列目录/建目录/重命名，宿主侧验证落盘
- [ ] 越狱断言：授权外绝对路径不可见（not found 语义）、dotdot 逃逸被拒
- [ ] 新缝测试进 CI（fixture wasm 预编译或按需构建，两者选一并说明取舍）
