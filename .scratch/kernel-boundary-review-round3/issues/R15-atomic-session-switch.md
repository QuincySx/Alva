# R15: 将日志与当前存储能力作为一次原子切换

**Priority:** P1
**Axis:** Spec / R10
**Origin:** 2026-09-11 review；HEAD `f9d0e57`
**Blocked by:** R18
**Status:** ready-for-agent

## 问题与证据

base_agent/agent.rs:265–268 在 state 锁内更新 st.session，释放锁后再调用 full_session.set。另一个线程可以在间隙开启 turn，读取新日志却仍向旧 store 写压缩或子会话；两个并发 swap 也可能使最终日志与 cell 不同。
R10 明确要求“避免与运行中的 turn 产生半切换状态”。共享 cell 解决了缓存旧引用，却没有解决切换原子性。证据：静态并发交错分析，未宣称已确定性复现调度窗口。

## 实现范围

由单一运行态同步边界保护旧会话 flush、日志与存储能力的切换及新的 turn 进入，结合 R18 避免将产品 store 所有权下沉内核。保留 flush 失败时不切换的语义，明确并发 swap 的串行化规则。

## 验收

- [ ] barrier/gate 确定性模拟 swap 与 turn 开始：任意时刻日志和存储属于同一会话。
- [ ] 两个并发 swap 后仍保持一致，不能用压力测试概率通过代替交错验收。
- [ ] flush 失败保留当前会话；与正在运行的压缩/子任务切换不会写错仓库。
- [ ] 顺序 new/resume/swap 与现有压缩测试通过。

## 执行记录

- Commit：待填写
- 实际验证命令及结果：待填写

遵守根/目录 AGENTS.md 与 FRACTAL-DOCS.md；Hub UI 修改前读取 UI glossary/wireframes/motion。只修本卡范围，不覆盖他人修改。行为问题先复现再修复，独立提交并同步公开快照；未运行的测试不得写为通过。本次 review 只建卡，不实施修复。
