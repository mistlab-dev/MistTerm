# 运行需本地 sshd 的集成测试（无 sshd 时相关用例自动 skip）
# 用法: .\scripts\run-integration-tests.ps1
# 环境变量: MISTTERM_TEST_SSH_HOST / PORT / USER / PASSWORD

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)
Set-Location $Root

$env:RUST_TEST_THREADS = "1"
$tests = @(
    "sftp_integration_test",
    "monitor_integration_test",
    "zmodem_integration_test"
)

foreach ($t in $tests) {
    Write-Host "==> cargo test --test $t -- --nocapture"
    cargo test --test $t --target-dir target/ci-test -- --nocapture
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

Write-Host "Integration tests finished."
