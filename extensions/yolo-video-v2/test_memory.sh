#!/bin/bash
# YOLO Video Extension 内存泄漏压力测试脚本

set -e

EXTENSION_ID="yolo-video-v2"
TEST_DURATION=${1:-300}  # 默认测试 300 秒
SAMPLE_INTERVAL=5  # 每 5 秒采样一次

echo "========================================"
echo "YOLO Video Extension 内存泄漏压力测试"
echo "========================================"
echo "测试时长：${TEST_DURATION} 秒"
echo "采样间隔：${SAMPLE_INTERVAL} 秒"
echo ""

# 查找扩展进程 PID
find_extension_pid() {
    pgrep -f "neomind.*extension.*$EXTENSION_ID" 2>/dev/null || echo ""
}

# 获取进程内存使用 (MB)
get_memory_mb() {
    local pid=$1
    if [ -n "$pid" ]; then
        ps -o rss= -p $pid 2>/dev/null | awk '{printf "%.1f", $1/1024}' || echo "0"
    else
        echo "0"
    fi
}

# 记录日志
log_file="memory_test_$(date +%Y%m%d_%H%M%S).log"
echo "日志文件：$log_file"
echo ""

# 等待扩展启动
echo "等待扩展启动..."
for i in {1..10}; do
    pid=$(find_extension_pid)
    if [ -n "$pid" ]; then
        echo "✓ 扩展进程已启动 (PID: $pid)"
        break
    fi
    sleep 1
done

if [ -z "$pid" ]; then
    echo "✗ 未找到扩展进程，请先启动扩展"
    exit 1
fi

# 开始监控
echo ""
echo "开始内存监控..."
echo "时间 (s) | 内存 (MB) | 增长 (MB)"
echo "---------|----------|----------"

start_time=$(date +%s)
initial_memory=$(get_memory_mb $pid)
prev_memory=$initial_memory
max_memory=$initial_memory
peak_time=0

echo "0       | $initial_memory | 0.0" | tee -a $log_file

while true; do
    sleep $SAMPLE_INTERVAL
    
    current_time=$(date +%s)
    elapsed=$((current_time - start_time))
    
    if [ $elapsed -ge $TEST_DURATION ]; then
        break
    fi
    
    current_memory=$(get_memory_mb $pid)
    growth=$(echo "$current_memory - $initial_memory" | bc)
    
    # 更新峰值
    if (( $(echo "$current_memory > $max_memory" | bc -l) )); then
        max_memory=$current_memory
        peak_time=$elapsed
    fi
    
    printf "%-8d | %-8.1f | %+-.1f\n" $elapsed $current_memory $growth | tee -a $log_file
    
    # 检查是否超过阈值
    if (( $(echo "$growth > 500" | bc -l) )); then
        echo ""
        echo "⚠️  警告：内存增长超过 500MB，可能存在泄漏！"
    fi
    
    prev_memory=$current_memory
done

# 最终统计
final_memory=$(get_memory_mb $pid)
total_growth=$(echo "$final_memory - $initial_memory" | bc)
avg_growth_per_sec=$(echo "scale=2; $total_growth / $TEST_DURATION" | bc)

echo ""
echo "========================================"
echo "测试结果汇总"
echo "========================================"
echo "初始内存：   $initial_memory MB"
echo "最终内存：   $final_memory MB"
echo "峰值内存：   $max_memory MB (在 ${peak_time}s)"
echo "总增长：     $total_growth MB"
echo "平均增长：   ${avg_growth_per_sec} MB/秒"
echo ""

# 评估结果
if (( $(echo "$total_growth < 50" | bc -l) )); then
    echo "✅ 通过：内存稳定，无明显泄漏"
    exit 0
elif (( $(echo "$total_growth < 200" | bc -l) )); then
    echo "⚠️  警告：有轻微内存增长，需关注"
    exit 1
else
    echo "❌ 失败：检测到内存泄漏 (${total_growth}MB)"
    exit 2
fi
