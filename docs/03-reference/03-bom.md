# BOM 与 SPDX

```sh
skill-bom bom --from lock --format json
skill-bom bom --from installed --format spdx-json
```

原生 BOM 包含根项目、包节点、直接及传递依赖边、请求约束、选定版本或 revision、来源链、摘要算法与证据范围、许可证声明、元数据完整性及补充声明。锁视图表示期望的已解析图，不标记为已部署；安装视图读取安装记录并验证当前内容。发现漂移时仍输出带诊断的 BOM，退出码为 1。

SPDX 输出固定为 2.3 JSON。根项目与 Skill 是 Package，文档通过 `DESCRIBES` 指向根项目，依赖边使用 `DEPENDS_ON`。未知许可证为 `NOASSERTION`，`filesAnalyzed` 为 `false`。归档摘要可以作为包校验和；自定义内容树摘要留在扩展说明中，不冒充归档摘要或 SPDX Package Verification Code。不会虚构 `pkg:skill` PURL。

同样的锁与固定生成时间能产出字节稳定的测试/CI BOM。ClawHub 扫描状态是历史观察，离线导出不会刷新远端状态。仓库测试用固定的 [SPDX 2.3 Schema](../../schemas/spdx-2.3.schema.json) 和关系引用完整性检查验证输出。

[上一篇：文件格式](02-file-formats.md) · [下一篇：贡献者指南](../04-contributor-guide/README.md)
