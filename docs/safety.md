# Safety & Risk Management

## Overview

This document outlines safety measures, denylists, and auto-merge policies for ftdata.

## Denylists

### Prohibited Actions

- **No destructive operations** on production data directories
- **No external API key exposure** in logs or error messages
- **No direct database modifications** outside of checkpoint module
- **No network requests** to untrusted third-party endpoints

### Data Denylist

```
# Sensitive paths that should never be modified
**/_checkpoints/
**/*.db
**/*.lock
```

## Auto-Merge Policy

### Requirements for Auto-Merge

| Check | Threshold |
|-------|-----------|
| Tests | All passing |
| Coverage | > 80% |
| Risk Score | < 0.5 |
| Documentation | Updated |
| No breaking changes | Required |

### Manual Review Required

- Changes to checkpoint database schema
- Network layer modifications
- Rate limiting logic changes
- File format conversion code

## MCP Scopes

### Allowed MCP Operations

- Read files within project
- Execute `ftdata` CLI commands
- Analyze data files (feather/parquet)
- Network requests to exchange APIs only

### Restricted MCP Operations

- File deletion outside of temp directories
- Database writes outside of checkpoint module
- External network requests (non-exchange)
- System command execution

## Incident Response

1. **Stop** - Halt any running operations
2. **Assess** - Check logs and checkpoint state
3. **Rollback** - Use `ftdata clean` if needed
4. **Report** - Document in issue tracker

## Security Best Practices

- Never log API keys or tokens
- Use checkpoint database for state, not file-based flags
- Validate all timestamp formats before processing
- Limit concurrent downloads to prevent rate limiting
