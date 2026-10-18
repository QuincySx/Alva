# L4 tool argument validation
> Replaceable default harness policy, explicitly usable with the standalone SDK Builder.

## 地位
Optional Plugin/Middleware implementation; the schema engine stays outside the standard-library-only SDK.
## 逻辑
Finalize compiles Register tools; OnRunStart includes finalizer tools. PrepareTool validates only ToolPreparationContext.ActualTool from the model invocation's frozen catalog and returns a model-recoverable error result for invalid input. A nil binding remains unknown/disallowed even if the live registry now has that name. BeforeTool validates that same actual execution again, so later preparation cannot bypass the execution guarantee. Same-name tool replacement becomes visible at the next model boundary and recompiles by schema hash without wrapping or changing actual tool identity.
Registration explicitly uses order10000 after ordinary preparation; no Priority implementation method or Configure lifecycle is required.
## 约束
No remote schema loader, argument coercion, defaults insertion or echoed validation values. Supports the schema library's draft-07/2020-12 semantics, with 1 MiB schemas, 4 MiB arguments and 128 cached schemas. The pinned santhosh-tekuri/jsonschema v6.0.3 compiler/validator receives json.Number for both schemas and arguments, preserving large integers, exact enum/const/bounds and decimal multiples. Numeric instances have at most4096 characters and exponent magnitude4096 to bound rational work. Schemas from later dynamic replacements are checked at use; the schema engine's format/annotation and unknown-keyword behavior is not a promise of arbitrary dialect support. Standalone SDK without this Plugin retains tool-owned validation. Prepared rejection avoids Execute; a later invalid rewrite is blocked before Execute but its previously committed intent remains truthful. No new kernel hooks or JSON schema dependencies.
## 业务域清单
| 文件 | 职责 |
|---|---|
| plugin.go | Compiler/cache and explicit Plugin lifecycle, prepared/frozen validation |
| plugin_test.go | Wrong type, required/extra fields, exact integers, late rewrite, invocation-frozen and next-boundary replacement, unknown binding, exact decimal/large schema bounds and denied local/remote schema regressions |
