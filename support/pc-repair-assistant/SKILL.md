---
name: repairing-computers
description: Diagnoses and repairs computer software and hardware problems like a digital mechanic. Covers CPU, RAM, disk, network, OS, drivers, BSOD, boot issues, performance optimization, and data recovery. Use for "컴퓨터 수리", "PC 문제", "블루스크린", "느린 컴퓨터", "computer repair", "troubleshoot", "BSOD", "slow PC" requests.
trigger-keywords: 컴퓨터 수리, PC 수리, 블루스크린, BSOD, 느린 컴퓨터, 하드웨어 진단, computer repair, troubleshoot, slow PC, crash, blue screen, hardware diagnostic
allowed-tools: Read, Write, Edit, Bash, Grep, Glob
priority: medium
tags: [agent, repair, troubleshooting, hardware, software, diagnostics]
---

# PC Repair Assistant

A digital mechanic that diagnoses and fixes computer software and hardware problems.

## Overview

**핵심 기능:**
- Hardware diagnostics (CPU, RAM, disk, GPU, thermals, PSU)
- Software troubleshooting (OS, drivers, crashes, BSOD, boot failures)
- Network diagnostics (connectivity, DNS, firewall, bandwidth)
- Performance optimization (startup, disk, memory, processes)
- Data recovery guidance and backup strategies
- Cross-platform support (Windows, macOS, Linux)

## When to Use

**명시적 요청:**
- "컴퓨터가 느려요", "블루스크린이 떠요"
- "부팅이 안 됩니다", "인터넷이 안 돼요"
- "하드디스크 소리가 이상해요", "팬 소음이 심해요"
- "My PC is slow", "Fix BSOD", "Computer won't start"

**자동 활성화:**
- System error logs detected
- Hardware diagnostic commands requested
- Performance analysis needed

## Diagnostic Workflow

### Step 1: Triage - Identify the Problem Category

| Symptom | Category | Priority |
|---------|----------|----------|
| Blue screen / kernel panic | OS / Driver | HIGH |
| Won't boot | Hardware / Boot | HIGH |
| Very slow | Performance | MEDIUM |
| No internet | Network | MEDIUM |
| Strange noises | Hardware | HIGH |
| Overheating | Thermal / Fan | HIGH |
| Random crashes | RAM / Driver | HIGH |
| Disk errors | Storage | HIGH |
| App won't open | Software | LOW |

### Step 2: Gather System Information

#### Windows
```powershell
# System overview
systeminfo
Get-ComputerInfo | Select-Object CsName, OsName, OsVersion, CsTotalPhysicalMemory

# CPU info
wmic cpu get name, numberofcores, maxclockspeed
Get-WmiObject Win32_Processor | Select-Object Name, NumberOfCores, MaxClockSpeed

# RAM info
wmic memorychip get capacity, speed, manufacturer
Get-WmiObject Win32_PhysicalMemory | Select-Object Capacity, Speed, Manufacturer

# Disk health
wmic diskdrive get status, model, size
Get-PhysicalDisk | Select-Object FriendlyName, MediaType, HealthStatus, Size

# GPU info
wmic path win32_videocontroller get name, driverversion, status

# Temperature (requires admin)
Get-WmiObject MSAcpi_ThermalZoneTemperature -Namespace "root/wmi" 2>$null
```

#### macOS
```bash
# System overview
system_profiler SPHardwareDataType

# CPU info
sysctl -n machdep.cpu.brand_string
sysctl -n hw.ncpu

# RAM info
sysctl -n hw.memsize | awk '{print $0/1073741824 " GB"}'

# Disk health
diskutil info disk0 | grep -E "SMART|Media Name|Disk Size"

# Temperature
sudo powermetrics --samplers smc -i1 -n1 2>/dev/null | grep -i temp

# GPU info
system_profiler SPDisplaysDataType
```

#### Linux
```bash
# System overview
uname -a && lsb_release -a 2>/dev/null

# CPU info
lscpu | grep -E "Model name|Core|Thread|MHz"

# RAM info
free -h && dmidecode -t memory 2>/dev/null | grep -E "Size|Speed|Manufacturer"

# Disk health (requires smartmontools)
sudo smartctl -a /dev/sda 2>/dev/null || lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT

# Temperature
sensors 2>/dev/null || cat /sys/class/thermal/thermal_zone*/temp 2>/dev/null

# GPU info
lspci | grep -i vga && nvidia-smi 2>/dev/null
```

### Step 3: Run Targeted Diagnostics

## Hardware Diagnostics

### CPU Issues
```bash
# Windows: CPU stress test check
wmic cpu get loadpercentage
# Check for throttling
powercfg /energy /duration 10

# macOS: CPU usage
top -l 1 | head -10

# Linux: CPU usage and temps
mpstat 1 5 2>/dev/null || top -bn1 | head -15
```

### RAM Issues
```bash
# Windows: Memory diagnostic
mdsched.exe  # Built-in memory diagnostic (requires reboot)
# Check for memory errors in event log
Get-WinEvent -LogName System -MaxEvents 50 | Where-Object {$_.Message -like "*memory*"}

# Linux: Memory test
sudo memtester 1G 1 2>/dev/null
# Check kernel memory errors
dmesg | grep -i "memory\|oom\|error"
```

### Disk Issues
```bash
# Windows: Disk check
chkdsk C: /F /R  # Requires admin, may need reboot
# SMART status
wmic diskdrive get status

# macOS: Disk repair
diskutil verifyDisk disk0
diskutil verifyVolume /

# Linux: Filesystem check (unmounted only!)
sudo fsck -n /dev/sda1
sudo smartctl -t short /dev/sda
```

### Thermal Issues
```bash
# Windows: Check thermal throttling
powercfg /energy /output energy_report.html

# macOS: Fan and thermal
sudo powermetrics --samplers smc -i1 -n3 2>/dev/null

# Linux: Temperature monitoring
watch -n 1 sensors 2>/dev/null
```

## Software Troubleshooting

### BSOD / Kernel Panic Analysis

#### Windows BSOD
```powershell
# Recent BSOD events
Get-WinEvent -LogName System -MaxEvents 100 | Where-Object {
    $_.Id -eq 1001 -or $_.Id -eq 41
} | Format-List TimeCreated, Message

# Analyze minidump files
ls C:\Windows\Minidump\*.dmp 2>$null

# Common BSOD codes
# IRQL_NOT_LESS_OR_EQUAL → Driver issue
# PAGE_FAULT_IN_NONPAGED_AREA → RAM or driver
# CRITICAL_PROCESS_DIED → System file corruption
# SYSTEM_SERVICE_EXCEPTION → Driver or update issue
```

#### Fix: System File Repair
```powershell
# Windows: SFC and DISM
sfc /scannow
DISM /Online /Cleanup-Image /RestoreHealth
```

### Boot Issues

```powershell
# Windows: Boot repair
bootrec /fixmbr
bootrec /fixboot
bootrec /rebuildbcd

# Safe mode boot
bcdedit /set {default} safeboot minimal
# Remove safe boot after fixing
bcdedit /deletevalue {default} safeboot
```

### Driver Issues

```powershell
# Windows: List problematic drivers
driverquery /v | findstr /i "degraded stopped"
Get-WmiObject Win32_PnPEntity | Where-Object { $_.Status -ne "OK" }

# Roll back driver
# Device Manager → right-click device → Properties → Driver → Roll Back

# Update all drivers
# Use manufacturer's support page or Windows Update
```

## Network Diagnostics

```bash
# Step 1: Check connectivity
ping 8.8.8.8        # Test internet (IP level)
ping google.com     # Test DNS resolution

# Step 2: Check DNS
nslookup google.com
# Try alternate DNS
nslookup google.com 8.8.8.8

# Step 3: Check route
traceroute google.com    # macOS/Linux
tracert google.com       # Windows

# Step 4: Check network config
ipconfig /all            # Windows
ifconfig                 # macOS/Linux
ip addr show             # Linux

# Step 5: Check firewall
# Windows
netsh advfirewall show allprofiles

# macOS
sudo pfctl -s rules 2>/dev/null

# Step 6: Speed test
curl -s https://raw.githubusercontent.com/sivel/speedtest-cli/master/speedtest.py | python3

# Step 7: Reset network stack (Windows)
netsh winsock reset
netsh int ip reset
ipconfig /flushdns
```

## Performance Optimization

### Startup Optimization
```powershell
# Windows: List startup programs
wmic startup list full
Get-CimInstance Win32_StartupCommand | Select-Object Name, Command, Location

# Disable unnecessary startup items
# Task Manager → Startup tab → Disable items

# macOS: List login items
osascript -e 'tell application "System Events" to get the name of every login item'

# Linux: List enabled services
systemctl list-unit-files --state=enabled
```

### Disk Cleanup
```powershell
# Windows: Disk cleanup
cleanmgr /d C:
# Clear temp files
Remove-Item -Path "$env:TEMP\*" -Recurse -Force -ErrorAction SilentlyContinue
Remove-Item -Path "C:\Windows\Temp\*" -Recurse -Force -ErrorAction SilentlyContinue

# macOS: Clear caches
sudo rm -rf ~/Library/Caches/* 2>/dev/null
sudo rm -rf /Library/Caches/* 2>/dev/null

# Linux: Clear package cache
sudo apt clean 2>/dev/null || sudo dnf clean all 2>/dev/null
```

### Process Management
```bash
# Find resource-hungry processes
# Windows
Get-Process | Sort-Object CPU -Descending | Select-Object -First 10
Get-Process | Sort-Object WorkingSet64 -Descending | Select-Object -First 10

# macOS / Linux
ps aux --sort=-%cpu | head -10
ps aux --sort=-%mem | head -10
```

## Data Recovery Guidance

| Situation | Tool | Platform |
|-----------|------|----------|
| Deleted files | Recuva, PhotoRec | Windows |
| Deleted files | Disk Drill, PhotoRec | macOS |
| Deleted files | TestDisk, PhotoRec | Linux |
| Corrupted partition | TestDisk | All |
| Failed drive | ddrescue (clone first!) | Linux |

### Emergency Steps
1. **STOP using the drive immediately** — continued use overwrites deleted data
2. Clone the drive first: `ddrescue /dev/sdX /path/to/image.img /path/to/log.txt`
3. Run recovery tools on the clone, never the original
4. For physically damaged drives: consult professional data recovery service

## Examples

### 예시 1: 블루스크린 진단

```
사용자: 컴퓨터에서 블루스크린이 자주 떠요

Claude:
1. 시스템 이벤트 로그 확인 (BSOD 코드 수집)
2. 미니덤프 파일 분석
3. 드라이버 상태 점검
4. RAM 진단 권장
5. 해결 방안 제시

결과:
- BSOD 코드: IRQL_NOT_LESS_OR_EQUAL
- 원인: 네트워크 드라이버 충돌
- 해결: 드라이버 업데이트 + SFC 실행
```

### 예시 2: 느린 컴퓨터 최적화

```
사용자: 컴퓨터가 너무 느려요

Claude:
1. 시스템 리소스 확인 (CPU, RAM, Disk 사용량)
2. 시작 프로그램 분석 (불필요한 항목 식별)
3. 디스크 상태 확인 (SSD/HDD 건강 상태)
4. 맬웨어 스캔 권장
5. 최적화 조치 실행

결과:
- 시작 프로그램 12개 → 4개로 축소
- 임시 파일 8.5GB 정리
- 디스크 조각 모음 (HDD의 경우)
- 부팅 시간: 45초 → 15초
```

### 예시 3: 네트워크 문제 해결

```
사용자: 인터넷이 안 됩니다

Claude:
1. IP 연결 테스트 (ping 8.8.8.8)
2. DNS 확인 (nslookup)
3. 네트워크 어댑터 상태 확인
4. 방화벽 규칙 점검
5. ISP 연결 확인

결과:
- DNS 서버 응답 없음
- 해결: DNS를 8.8.8.8, 8.8.4.4로 변경
- 네트워크 스택 초기화
```

## Best Practices

**DO:**
- Always gather system info before attempting fixes
- Back up data before major repairs (disk fixes, OS repair)
- Start with the simplest solution first (reboot, update, clean)
- Check event logs and crash dumps for root cause analysis
- Document what was changed for rollback capability

**DON'T:**
- Run disk repair tools on mounted/in-use partitions
- Force kill system processes without understanding dependencies
- Delete system files or registry entries without backups
- Assume hardware failure without software diagnostics first
- Skip SMART checks when disk issues are suspected

## Troubleshooting Matrix

| Problem | Quick Check | Likely Cause | Fix |
|---------|------------|--------------|-----|
| Random reboots | Event log | RAM / PSU / Heat | memtest, clean fans |
| Slow boot | Startup list | Too many startups | Disable unnecessary |
| App crashes | Event log | Corrupt files | Reinstall / SFC |
| No display | Monitor cable | GPU / Cable | Reseat GPU, try other port |
| Beep codes | Count beeps | RAM / GPU | Reseat components |
| Disk clicking | SMART check | Dying drive | Backup immediately! |
| Fan loud | Temp monitor | Dust / thermal paste | Clean / reapply paste |
| USB not found | Device mgr | Driver / power | Update driver, try other port |
