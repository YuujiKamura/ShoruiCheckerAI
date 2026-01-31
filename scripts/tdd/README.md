# TDD Issue-driven Flow

## 1. Create Issue
Use the **TDD Issue** template in GitHub Issues.

## 2. Export issue body
Copy the issue body and save it locally as `issue.md`.

## 3. Generate test stubs
Run:
```
npm run tdd:issue -- path/to/issue.md
```

This will create test stubs for entries listed under `先に書くテスト`.

## 3b. Fetch from GitHub Issues (optional)
If you use GitHub Issues, you can fetch the body directly:
```
npm run tdd:issue:gh -- ISSUE_NUMBER
```

## 4. Run tests
```
npm run test
```

## Notes
- Frontend tests: `tests/frontend/*.test.js`
- Rust tests: `src-tauri/tests/*.rs`
