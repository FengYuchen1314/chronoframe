<script setup lang="ts">
import { Alert as AAlert, Badge as ABadge, Button as AButton, Drawer as ADrawer, Progress as AProgress, Space as ASpace, Table as ATable, Tag as ATag } from 'ant-design-vue'
const queue = useAdminUploads()
const { state, open, active, queued, pending, failed, done } = queue
const notice = useAdminNotice()
const columns = [{ title: '文件 / 相册', key: 'file' }, { title: '状态', key: 'status', width: 160 }, { title: '操作', key: 'action', width: 75 }]
const labels = { queued: '等待上传', uploading: '上传与处理', done: '已入库', failed: '未确认成功' }
const colors = { queued: 'default', uploading: 'processing', done: 'success', failed: 'error' }
const retry = async () => {
  if (await notice.confirm('失败请求可能已经入库，请先核对对应相册，避免重复上传。确认重新上传所有失败文件？')) queue.retryFailed()
}
useEventListener('beforeunload', (event: BeforeUnloadEvent) => {
  if (pending.value || failed.value) { event.preventDefault(); event.returnValue = '' }
})
</script>

<template>
  <ABadge :dot="failed > 0"><AButton @click="open = true"><Icon name="tabler:cloud-upload" /> 上传队列<span v-if="pending">（{{ pending }}）</span></AButton></ABadge>
  <ADrawer v-model:open="open" title="上传队列" width="min(720px, 100vw)" :destroy-on-close="false">
    <AAlert type="info" show-icon message="可切换后台页面，上传会继续" description="7 个并发任务，文件始终上传到选择时的相册。不要刷新或关闭浏览器；离线时队列不会自动重试。" />
    <div class="admin-upload-summary"><span>已入库 <strong>{{ done }}</strong></span><span>处理中 {{ active }}</span><span>待上传 {{ queued }}</span><span v-if="failed">未确认 {{ failed }}</span></div>
    <AProgress v-if="state.items.length" :percent="Math.round(done / state.items.length * 100)" :status="failed ? 'exception' : undefined" />
    <ASpace wrap class="mb-4">
      <AButton v-if="pending" @click="state.paused ? queue.resume() : queue.pause()">{{ state.paused ? '继续上传' : '暂停队列' }}</AButton>
      <AButton v-if="failed" @click="retry">重试失败文件</AButton>
      <AButton :disabled="!done" @click="queue.clearDone">清除已完成记录</AButton>
    </ASpace>
    <AAlert v-if="state.paused" type="warning" show-icon message="已暂停新任务，正在上传的文件会继续完成" class="mb-4" />
    <ATable :columns="columns" :data-source="state.items" row-key="id" size="small" :pagination="{ pageSize: 20, showSizeChanger: false }" :scroll="{ x: 480 }">
      <template #bodyCell="{ column, record }">
        <template v-if="column.key === 'file'"><div class="admin-file-name">{{ record.name }}</div><NuxtLink :to="{ path: '/dashboard/albums', query: { album: record.albumId } }" @click="open = false">{{ record.albumName }}</NuxtLink><span class="admin-help"> · {{ adminBytes(record.size) }}</span><div v-if="record.error" class="admin-field-error">{{ record.error }}</div></template>
        <template v-if="column.key === 'status'"><ATag :color="colors[record.status as keyof typeof colors]">{{ labels[record.status as keyof typeof labels] }}</ATag></template>
        <AButton v-if="column.key === 'action' && record.status !== 'uploading'" type="link" size="small" @click="queue.remove(record.id)">{{ record.status === 'queued' ? '取消' : '移除' }}</AButton>
      </template>
    </ATable>
    <p class="admin-help">取消仅移除尚未开始的队列项；移除记录和清除记录均不会删除已入库的图片。</p>
  </ADrawer>
</template>
