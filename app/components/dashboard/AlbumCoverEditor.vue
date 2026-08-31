<script setup lang="ts">
import { Alert as AAlert, Button as AButton, Card as ACard, Empty as AEmpty, InputSearch as AInputSearch, Modal as AModal, Pagination as APagination, Popconfirm as APopconfirm, Space as ASpace, Tag as ATag, Upload as AUpload } from 'ant-design-vue'
import type { Album, AlbumCover, Photo } from '~/types/dashboard'

const props = defineProps<{ album: Album; photos: Photo[]; disabled: boolean }>()
const emit = defineEmits<{ saved: [id: string, cover: AlbumCover]; busy: [value: boolean] }>()
const { adminFetch } = useAdminApi()
const toast = useAdminNotice()
const busy = ref(false)
const pickerOpen = ref(false)
const selectedId = ref<string | null>(null)
const query = ref('')
const page = ref(1)
const error = ref('')
const pageSize = 24
const filtered = computed(() => props.photos.filter(photo => photo.originalName.toLocaleLowerCase().includes(query.value.trim().toLocaleLowerCase())))
const pagePhotos = computed(() => filtered.value.slice((page.value - 1) * pageSize, page.value * pageSize))
const selectedPhoto = computed(() => props.photos.find(photo => photo.id === selectedId.value))
const coverLabel = computed(() => props.album.coverSource === 'upload' ? '单独上传' : props.album.coverSource === 'photo' ? '相册选图' : '自动封面')
watch(query, () => { page.value = 1 })

const openPicker = () => {
  if (props.disabled || busy.value) return
  selectedId.value = props.album.coverPhotoId
  query.value = ''
  page.value = 1
  error.value = ''
  pickerOpen.value = true
}
const save = async (method: 'POST' | 'PUT' | 'DELETE', body?: FormData | { photoId: string }) => {
  if (props.disabled || busy.value) return
  const albumId = props.album.id
  busy.value = true
  emit('busy', true)
  error.value = ''
  try {
    const cover = await adminFetch<AlbumCover>(`/api/albums/${encodeURIComponent(albumId)}/cover`, { method, body })
    emit('saved', albumId, cover)
    clearNuxtData(['public-albums', 'album-detail'])
    pickerOpen.value = false
    toast.add({ title: method === 'DELETE' ? '已恢复自动封面' : '相册封面已更新', color: 'success' })
  } catch (cause) {
    error.value = getAdminApiErrorMessage(cause)
    toast.add({ title: '封面保存失败，请刷新确认后重试', description: error.value, color: 'error' })
  } finally {
    busy.value = false
    emit('busy', false)
  }
}
const upload = (file: File) => {
  if (!/\.(png|jpe?g|webp)$/i.test(file.name)) {
    error.value = '请选择 PNG、JPG/JPEG 或 WebP 图片'
    return false
  }
  const form = new FormData()
  form.append('file', file)
  void save('POST', form)
  return false
}
const confirmSelection = () => { if (selectedPhoto.value) void save('PUT', { photoId: selectedPhoto.value.id }) }
</script>

<template>
  <ACard title="相册封面">
    <template #extra><ATag :color="album.coverSource === 'auto' ? 'default' : 'blue'">{{ coverLabel }}</ATag></template>
    <div class="cover-editor">
      <div class="cover-preview" :aria-busy="busy">
        <img v-if="album.coverUrl" :src="album.coverUrl" :alt="album.name + '的当前封面'" />
        <AEmpty v-else :image="AEmpty.PRESENTED_IMAGE_SIMPLE" description="暂无封面" />
      </div>
      <div class="cover-controls">
        <p class="cover-heading">为相册选一张封面</p>
        <p class="cover-help">用于相册首页卡片和详情页背景。选择后立即保存，不改变相册内图片的顺序。</p>
        <ASpace wrap>
          <AButton type="primary" :disabled="disabled || !photos.length" @click="openPicker">从相册选择</AButton>
          <AUpload accept=".png,.jpg,.jpeg,.webp" :show-upload-list="false" :multiple="false" :before-upload="upload" :disabled="disabled">
            <AButton :loading="busy" :disabled="disabled && !busy">从电脑上传</AButton>
          </AUpload>
          <APopconfirm title="恢复自动封面？" description="只移除手动封面设置，不会删除相册里的照片。" ok-text="恢复" cancel-text="取消" :disabled="disabled || album.coverSource === 'auto'" @confirm="save('DELETE')">
            <AButton :disabled="disabled || album.coverSource === 'auto'">恢复自动封面</AButton>
          </APopconfirm>
        </ASpace>
        <p class="cover-help">支持 PNG、JPG/JPEG、WebP。单独上传的封面不计入照片数量，也不包含在下载包中。</p>
        <p v-if="busy" role="status" class="cover-help">正在上传并保存封面，请稍候…</p>
      </div>
    </div>
    <AAlert v-if="error && !pickerOpen" type="error" show-icon :message="error" class="mt-4" />
  </ACard>

  <AModal v-model:open="pickerOpen" title="从相册选择封面" :width="800" ok-text="设为封面" cancel-text="取消" :confirm-loading="busy" :ok-button-props="{ disabled: !selectedPhoto || busy }" :cancel-button-props="{ disabled: busy }" :closable="!busy" :keyboard="!busy" :mask-closable="!busy" @ok="confirmSelection">
    <AInputSearch v-model:value="query" placeholder="搜索图片文件名" aria-label="搜索封面图片" allow-clear class="mb-4" :disabled="busy" />
    <AAlert v-if="error" type="error" show-icon :message="error" class="mb-4" />
    <div v-if="pagePhotos.length" class="cover-picker" aria-label="可选封面图片">
      <button v-for="photo in pagePhotos" :key="photo.id" type="button" class="cover-choice" :class="{ chosen: selectedId === photo.id }" :aria-pressed="selectedId === photo.id" :aria-label="'选择封面：' + photo.originalName" :disabled="busy" @click="selectedId = photo.id">
        <img :src="`/api/photos/${encodeURIComponent(photo.id)}/thumbnail?v=grid2`" alt="" loading="lazy" draggable="false" />
        <span class="cover-filename" :title="photo.originalName">{{ photo.originalName }}</span>
        <span v-if="selectedId === photo.id" class="cover-check" aria-hidden="true">✓</span>
      </button>
    </div>
    <AEmpty v-else description="没有匹配的图片" />
    <div class="cover-picker-footer">
      <span class="cover-selection">{{ selectedPhoto ? `已选择：${selectedPhoto.originalName}` : '请选择一张图片作为封面' }}</span>
      <APagination v-model:current="page" :total="filtered.length" :page-size="pageSize" :show-size-changer="false" size="small" :disabled="busy" hide-on-single-page />
    </div>
  </AModal>
</template>

<style scoped>
.cover-editor { display: flex; gap: 24px; align-items: center; }
.cover-preview { width: 240px; aspect-ratio: 4 / 3; flex-shrink: 0; border-radius: 8px; overflow: hidden; background: #f5f5f5; display: grid; place-items: center; border: 1px solid #f0f0f0; }
.cover-preview img { width: 100%; height: 100%; object-fit: cover; }
.cover-controls { min-width: 0; }
.cover-heading { font-size: 16px; font-weight: 600; margin: 0 0 8px; }
.cover-help { color: #737373; line-height: 1.7; margin: 8px 0 16px; font-size: 13px; }
.cover-help:last-child { margin-bottom: 0; }
.cover-picker { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 12px; max-height: 52vh; overflow-y: auto; padding: 3px; }
.cover-choice { position: relative; min-width: 0; overflow: hidden; border: 2px solid transparent; border-radius: 8px; background: #f5f5f5; padding: 0; cursor: pointer; text-align: left; }
.cover-choice img { width: 100%; aspect-ratio: 4 / 3; object-fit: cover; display: block; }
.cover-choice:hover { border-color: #91caff; }
.cover-choice.chosen, .cover-choice:focus-visible { border-color: #1677ff; outline: 2px solid #bae0ff; outline-offset: 1px; }
.cover-choice:disabled { cursor: wait; }
.cover-filename { display: block; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; padding: 7px; font-size: 12px; }
.cover-check { position: absolute; top: 6px; right: 6px; border-radius: 50%; width: 24px; height: 24px; display: grid; place-items: center; background: #1677ff; color: white; }
.cover-picker-footer { display: flex; flex-wrap: wrap; gap: 12px; justify-content: space-between; margin-top: 16px; }
.cover-selection { color: #737373; overflow-wrap: anywhere; font-size: 13px; }
@media (max-width: 640px) { .cover-editor { flex-direction: column; align-items: stretch; gap: 16px; } .cover-preview { width: 100%; max-width: 320px; } .cover-picker { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
</style>
