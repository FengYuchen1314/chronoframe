<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Card as ACard, CheckboxGroup as ACheckboxGroup, Form as AForm, FormItem as AFormItem, InputNumber as AInputNumber, Select as ASelect, Switch as ASwitch, Table as ATable, Tag as ATag, Space as ASpace, Progress as AProgress, Popconfirm as APopconfirm, Statistic as AStatistic, Modal as AModal, RadioGroup as ARadioGroup } from 'ant-design-vue'
import type { AdminAlbumDownloads, AlbumDownloadSettings, DownloadFormat } from '~~/shared/types/downloads'
definePageMeta({ layout: 'dashboard' })
useHead({ title: '下载管理' })
const { adminFetch } = useAdminApi()
const notice = useAdminNotice()
const route = useRoute()
const data = ref<AdminAlbumDownloads>({ settings: [], jobs: [], localBytes: 0, directory: 'data/album-downloads' })
const selected = ref(typeof route.query.album === 'string' ? route.query.album : '')
const draft = reactive({ enabled: false, formats: ['webp'] as DownloadFormat[], imageMB: 5 })
const saved = ref('')
const loading = ref(false)
const saving = ref(false)
const bulkOpen = ref(false)
const bulkSaving = ref(false)
const bulkScope = ref<'selected' | 'all'>('selected')
const bulkSelected = ref<string[]>([])
const bulkDraft = reactive({ enabled: false, formats: ['webp'] as DownloadFormat[], imageMB: 5 })
const bulkBaseline = ref('')
const bulkSignature = computed(() => JSON.stringify([bulkScope.value, bulkSelected.value, bulkDraft]))
const bulkDirty = computed(() => bulkOpen.value && bulkBaseline.value !== bulkSignature.value)
const bulkCount = computed(() => bulkScope.value === 'all' ? data.value.settings.length : bulkSelected.value.length)
const actionId = ref('')
const error = ref('')
const signature = computed(() => JSON.stringify(draft))
const dirty = computed(() => !!saved.value && signature.value !== saved.value)
const current = computed(() => data.value.settings.find(item => item.albumId === selected.value))
const jobs = computed(() => data.value.jobs.filter(job => job.albumId === selected.value))
const options = computed(() => data.value.settings.map(item => ({ value: item.albumId, label: item.albumName })))
const formats = ['png', 'jpg', 'jpeg', 'webp'].map(value => ({ label: value.toUpperCase(), value }))
const columns = [{ title: '格式', dataIndex: 'format', width: 80 }, { title: '状态 / 进度', key: 'status', width: 180 }, { title: '文件大小', key: 'size', width: 105 }, { title: '生成时间', key: 'created', width: 155 }, { title: '操作', key: 'actions', width: 160 }]
let timer: ReturnType<typeof setTimeout> | undefined
let mounted = false
let loadRequest: Promise<void> | undefined
const apply = (config?: AlbumDownloadSettings) => {
  if (!config) return
  Object.assign(draft, { enabled: config.enabled, formats: [...config.formats], imageMB: config.maxImageBytes / 1_000_000 })
  saved.value = signature.value
}
const load = async (reset = false): Promise<void> => {
  if (loadRequest) { await loadRequest; if (reset) return load(true); return }
  loading.value = true
  loadRequest = (async () => {
    try {
      data.value = await adminFetch<AdminAlbumDownloads>('/api/album-downloads')
      error.value = ''
      if (!data.value.settings.some(item => item.albumId === selected.value)) { selected.value = data.value.settings[0]?.albumId || ''; reset = true }
      if (reset || !saved.value) apply(current.value)
    } catch (cause) { error.value = getAdminApiErrorMessage(cause) }
    finally { loading.value = false; loadRequest = undefined }
  })()
  await loadRequest
}
const pick = async (value: unknown) => {
  if (dirty.value && !await notice.confirm('下载设置尚未保存，确定放弃修改并切换相册吗？')) return
  selected.value = String(value)
  apply(current.value)
}
const save = async () => {
  if (!current.value || saving.value) return
  if (!draft.formats.length) { notice.add({ title: '请选择至少一种图片格式', color: 'warning' }); return }
  if (!draft.enabled && current.value.enabled && !await notice.confirm('关闭公开下载并删除该相册已生成的本地 ZIP？原始图片不受影响。', true)) return
  saving.value = true
  try {
    await adminFetch(`/api/albums/${selected.value}/download-settings`, { method: 'PUT', body: { enabled: draft.enabled, formats: draft.formats, maxImageBytes: Math.round((draft.imageMB || 0) * 1_000_000), maxZipBytes: 0 } })
    saved.value = signature.value
    await load(true)
    notice.add({ title: draft.enabled ? '已保存，系统将在后台生成压缩包' : '已关闭公开下载，本地压缩包将自动清理', color: 'success' })
  } catch (cause) { notice.add({ title: '保存失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { saving.value = false }
}
const openBulk = () => {
  if (!current.value || saving.value) return
  if (dirty.value) { notice.add({ title: '请先保存或重置当前相册的修改', color: 'warning' }); return }
  bulkScope.value = 'selected'
  bulkSelected.value = [selected.value]
  Object.assign(bulkDraft, { enabled: current.value.enabled, formats: [...current.value.formats], imageMB: current.value.maxImageBytes / 1_000_000 })
  bulkBaseline.value = bulkSignature.value
  bulkOpen.value = true
}
const closeBulk = async () => {
  if (bulkSaving.value) return
  if (bulkDirty.value && !await notice.confirm('批量设置尚未应用，确定放弃修改吗？')) return
  bulkOpen.value = false
}
const saveBulk = async () => {
  if (bulkSaving.value) return
  if (!bulkCount.value) { notice.add({ title: '请至少选择一个相册', color: 'warning' }); return }
  if (!bulkDraft.formats.length) { notice.add({ title: '请选择至少一种图片格式', color: 'warning' }); return }
  if (bulkScope.value === 'selected' && bulkSelected.value.some(id => !data.value.settings.some(item => item.albumId === id))) {
    notice.add({ title: '选中的相册已不存在，请重新选择', color: 'warning' }); return
  }
  const settings = { enabled: bulkDraft.enabled, formats: [...bulkDraft.formats], maxImageBytes: Math.round((bulkDraft.imageMB || 0) * 1_000_000), maxZipBytes: 0 }
  const target = bulkScope.value === 'all' ? { scope: 'all' } : { scope: 'selected', albumIds: [...bulkSelected.value] }
  const scope = bulkScope.value === 'all' ? `全部现有相册（当前 ${bulkCount.value} 个）` : `选中的 ${bulkCount.value} 个相册`
  bulkSaving.value = true
  try {
    if (!await notice.confirm(`将覆盖${scope}的下载开关、图片格式和大小上限：${settings.enabled ? '开启下载' : '关闭下载'}，${settings.formats.join(' / ').toUpperCase()}，单张${settings.maxImageBytes ? `最多 ${bulkDraft.imageMB} MB` : '不限大小'}。旧 ZIP 将清理${settings.enabled ? '并在后台重新生成' : ''}，原始图片不受影响。确定应用？`, true)) return
    const result = await adminFetch<{ updated: number }>('/api/album-downloads/settings/bulk', { method: 'PUT', body: { target, settings } })
    bulkOpen.value = false
    await load(true)
    notice.add({ title: `已统一设置 ${result.updated} 个相册`, description: settings.enabled ? '压缩包将在后台自动更新，可以离开页面。' : '已关闭公开下载，本地 ZIP 将自动清理；原始图片不受影响。', color: 'success' })
  } catch (cause) { notice.add({ title: '批量设置失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { bulkSaving.value = false }
}
const rebuild = async () => {
  if (!current.value?.enabled || saving.value) return
  if (dirty.value) { notice.add({ title: '请先保存或放弃下载设置', color: 'warning' }); return }
  saving.value = true
  try {
    await adminFetch(`/api/albums/${selected.value}/downloads/rebuild`, { method: 'POST' })
    await load()
    notice.add({ title: '已提交重新生成任务，可以离开页面', color: 'success' })
  } catch (cause) { notice.add({ title: '提交失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { saving.value = false }
}
const jobAction = async (id: string, action: 'cancel' | 'delete') => {
  if (actionId.value) return
  actionId.value = id
  try {
    await adminFetch(`/api/album-downloads/${id}${action === 'cancel' ? '/cancel' : ''}`, { method: action === 'cancel' ? 'POST' : 'DELETE' })
    await load()
    notice.add({ title: action === 'cancel' ? '已请求取消任务' : '已撤下下载，正在删除本地 ZIP', color: 'success' })
  } catch (cause) { notice.add({ title: '操作失败', description: getAdminApiErrorMessage(cause), color: 'error' }) }
  finally { actionId.value = '' }
}
const poll = async () => { await load(); if (mounted) timer = setTimeout(poll, document.hidden ? 15000 : 3000) }
onMounted(() => { mounted = true; void poll() })
onBeforeUnmount(() => { mounted = false; clearTimeout(timer) })
onBeforeRouteLeave(() => (!dirty.value && !bulkDirty.value) || notice.confirm('下载设置尚未保存，确定离开吗？'))
useEventListener('beforeunload', (event: BeforeUnloadEvent) => { if (dirty.value || bulkDirty.value) { event.preventDefault(); event.returnValue = '' } })
</script>

<template>
  <div>
    <DashboardPageHeader title="下载管理" description="为相册发布预先生成的 ZIP。文件仅保存在本机，不占用 S3 或 WebDAV。"><AButton :loading="loading" @click="load()">刷新</AButton></DashboardPageHeader>
    <div class="admin-stack">
      <AAlert v-if="error" type="error" show-icon :message="error" />
      <ACard size="small"><div class="admin-toolbar" style="margin:0"><ASpace wrap><span>管理相册</span><ASelect :value="selected" :options="options" :disabled="saving" show-search option-filter-prop="label" aria-label="管理相册" placeholder="请选择相册" style="width:260px;max-width:65vw" @change="pick" /><AButton :disabled="!current || saving" @click="openBulk">批量设置</AButton></ASpace><AStatistic title="本地 ZIP 占用" :value="adminBytes(data.localBytes)" :value-style="{ fontSize: 20 }" /></div></ACard>
      <AAlert v-if="!data.settings.length && !loading" type="info" show-icon message="请先创建相册，再配置公开下载。" />
      <div v-if="current" class="admin-settings-grid">
        <ACard title="下载设置">
          <AForm name="album-download-settings" layout="vertical" :model="draft" :disabled="saving" @finish="save">
            <AFormItem label="可供下载" name="enabled" extra="开启后，系统自动生成所选格式的压缩包。"><ASwitch v-model:checked="draft.enabled" checked-children="开启" un-checked-children="关闭" /></AFormItem>
            <AFormItem label="图片格式" name="formats" required extra="每种格式生成一个独立 ZIP。JPG 与 JPEG 编码相同，扩展名不同。"><ACheckboxGroup v-model:value="draft.formats" :options="formats" /></AFormItem>
            <AFormItem label="单张图片大小上限（MB）" name="imageMB" extra="0 表示不限。必要时降低画质或分辨率；PNG 保持无损编码，通过缩小尺寸达标。"><AInputNumber v-model:value="draft.imageMB" :min="0" :max="500" :step="0.5" /></AFormItem>
            <ASpace><AButton type="primary" html-type="submit" :loading="saving">保存设置</AButton><AButton :disabled="!dirty || saving" @click="apply(current)">重置</AButton></ASpace>
            <p v-if="dirty" class="admin-help mt-3">有未保存的修改</p>
          </AForm>
        </ACard>
        <ACard title="本地压缩包" :body-style="{ padding: '16px' }">
          <template #extra><AButton :disabled="!current.enabled" :loading="saving" @click="rebuild">重新生成</AButton></template>
          <AAlert type="info" show-icon message="任务在后台运行，可随时离开页面" description="相册增删图片或修改名称后自动更新。只有完整生成的当前版本可供下载；旧包和失败临时文件自动清理。" class="mb-4" />
          <ATable :columns="columns" :data-source="jobs" row-key="id" size="middle" :pagination="{ pageSize: 8, showSizeChanger: false }" :scroll="{ x: 700 }">
            <template #bodyCell="{ column, record }">
              <template v-if="column.dataIndex === 'format'"><strong>{{ record.format.toUpperCase() }}</strong><div class="admin-help">v{{ record.revision }}</div></template>
              <template v-else-if="column.key === 'status'"><ATag :color="downloadStatusColor[record.status]">{{ downloadStatus[record.status] || record.status }}</ATag><AProgress v-if="record.status === 'running'" :percent="record.total ? Math.round(record.completed / record.total * 100) : 0" size="small" /><div v-if="record.error" class="admin-help" style="color:#cf1322">{{ record.error }}</div></template>
              <template v-else-if="column.key === 'size'">{{ record.byteSize ? adminBytes(record.byteSize) : '—' }}</template>
              <template v-else-if="column.key === 'created'">{{ new Date(record.createdAt * 1000).toLocaleString('zh-CN', { hour12: false }) }}</template>
              <template v-else-if="column.key === 'actions'"><ASpace>
                <AButton v-if="record.status === 'ready' && record.revision === current.revision && current.enabled" type="link" size="small" :href="`/api/albums/${record.albumId}/downloads/${record.format}?version=${record.id}`">下载</AButton>
                <AButton v-if="['queued','running'].includes(record.status)" type="link" size="small" :loading="actionId === record.id" @click="jobAction(record.id, 'cancel')">取消</AButton>
                <APopconfirm v-if="!['deleted','deleting'].includes(record.status)" title="删除本地压缩包？原始图片不会删除，可随时重新生成。" ok-text="删除" :ok-button-props="{ danger: true }" @confirm="jobAction(record.id, 'delete')"><AButton type="link" danger size="small" :loading="actionId === record.id">删除</AButton></APopconfirm>
              </ASpace></template>
            </template>
          </ATable>
          <p class="admin-help mt-3">存放位置：{{ data.directory }}。删除后当前版本不会自动重建，点击“重新生成”即可恢复。</p>
        </ACard>
      </div>
    </div>
    <AModal :open="bulkOpen" title="批量设置相册下载" :width="640" :footer="null" :closable="!bulkSaving" :mask-closable="!bulkSaving" :keyboard="!bulkSaving" @cancel="closeBulk">
      <AAlert type="warning" show-icon message="统一覆盖各相册的单独设置" description="下载开关、图片格式和大小上限都会替换为下方的设置，不会合并。只影响本次指定的现有相册，之后新建的相册不受影响。" class="mb-5" />
      <AForm name="bulk-download-settings" layout="vertical" :model="bulkDraft" :disabled="bulkSaving" @finish="saveBulk">
        <AFormItem label="应用范围">
          <ARadioGroup v-model:value="bulkScope" aria-label="应用范围" :options="[{ label: '选定相册', value: 'selected' }, { label: `全部相册（${data.settings.length}）`, value: 'all' }]" />
        </AFormItem>
        <AFormItem v-if="bulkScope === 'selected'" label="选择相册" required>
          <ASelect v-model:value="bulkSelected" mode="multiple" :options="options" show-search option-filter-prop="label" aria-label="选择相册" allow-clear placeholder="搜索并选择一个或多个相册" :max-tag-count="4" />
        </AFormItem>
        <p class="admin-help mb-5">将覆盖 {{ bulkCount }} 个相册。以下默认带入当前相册已保存的设置，可按需修改。</p>
        <AFormItem label="可供下载" name="enabled" extra="开启后自动生成 ZIP；关闭后撤下下载并清理本地 ZIP，原始图片不受影响。"><ASwitch v-model:checked="bulkDraft.enabled" checked-children="开启" un-checked-children="关闭" /></AFormItem>
        <AFormItem label="图片格式" name="formats" required extra="每种格式生成一个独立 ZIP。"><ACheckboxGroup v-model:value="bulkDraft.formats" :options="formats" /></AFormItem>
        <AFormItem label="单张图片大小上限（MB）" name="imageMB" extra="0 表示不限。必要时降低画质或缩小尺寸，不会跳过超限图片。"><AInputNumber v-model:value="bulkDraft.imageMB" :min="0" :max="500" :step="0.5" /></AFormItem>
        <div style="display:flex;justify-content:flex-end;gap:8px"><AButton :disabled="bulkSaving" @click="closeBulk">取消</AButton><AButton type="primary" html-type="submit" :loading="bulkSaving" :disabled="!bulkCount || !bulkDraft.formats.length">覆盖并应用（{{ bulkCount }}）</AButton></div>
      </AForm>
    </AModal>
  </div>
</template>
