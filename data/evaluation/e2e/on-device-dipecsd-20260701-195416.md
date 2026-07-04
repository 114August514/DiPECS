# 设备内 dipecsd 真机回路验证

- 时间:2026-07-01 19:54:16
- 设备:emulator-5554(ABI=x86_64 SDK=35)
- 二进制:target/x86_64-linux-android/release/dipecsd(target=x86_64-linux-android,NDK 交叉编译)
- 数据源:设备内 dipecsd(交叉编译 x86_64-linux-android)+ localhost bridge,无 adb forward
- 采集样本:data/traces/android_real_device_sample.redacted.jsonl
- 设备确认派发数(dipecsd 运行时 trace):2
- app execute_ok 增量:1(=19-18)
- 判定:LOOP-CLOSED

## 与 action-loop-e2e 的区别

action-loop-e2e 里 dipecsd 跑在主机、经 adb forward 转发,adb 代理层的数据/FIN
竞态会截断回执;本场景把 dipecsd 推进设备内直连 127.0.0.1 的 app 动作 socket,
无 adb forward、无代理失真,是最接近生产(/system/bin/dipecsd)的真机回路。
