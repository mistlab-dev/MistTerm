# MistDocs / 团队文档检索（客户端契约）

> 状态：v1.1.3 客户端已接入；服务端未实现时 **404 / 501 → 空结果**，UI 回退到团队/个人片段检索。

## 端点

```
GET /v1/teams/{team_id}/docs/search?q={query}&limit={1..20}
Authorization: Bearer <access_token>
```

## 成功响应 `200`

```json
{
  "items": [
    {
      "id": "doc-uuid",
      "title": "清理生产日志",
      "excerpt": "使用 find … -mtime +7 -delete，禁止 rm -rf /var/log。",
      "slug": "safe-cleanup",
      "score": 12
    }
  ]
}
```

| 字段 | 说明 |
|------|------|
| `id` | 文档稳定 ID |
| `title` | 文档或章节标题 |
| `excerpt` | 可展示段落（客户端作 `KnowledgeHit.body`） |
| `slug` | 段落锚点；客户端拼 `doc:{id}#{slug}` |
| `score` | 服务端相关度（可选，默认 0） |

## 软失败

| 状态 | 客户端行为 |
|------|------------|
| `404` / `501` | `Ok(None)`，不报错 |
| 其它 4xx/5xx | 返回 `TeamApiError`（Ask UI 可忽略并仅用片段） |
| 未登录 | 不调用 |

## 客户端锚点

- 片段：`fragment:{id}`
- 文档：`doc:{id}` 或 `doc:{id}#{slug}`
- 模型兜底：`model:fallback`
