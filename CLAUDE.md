@AGENTS.md

# Claude Code — aterm v3

## Claude 전용 설정

- 세션 ID: `aigentry-aterm-claude`
- 보고: `telepty inject --ref --submit --from aigentry-aterm-claude aigentry-orchestrator-claude "보고 내용"`
- 3회 실패 시 위임: `telepty allow --id aigentry-aterm-codex codex resume`
- 헌법: `~/projects/aigentry/docs/CONSTITUTION.md`
