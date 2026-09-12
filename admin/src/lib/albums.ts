// 相册表单相关的纯函数。逐字移植自 shared/utils/admin-albums.ts。
import type { Album, AlbumDraft } from './types'

export function albumDraftOf(album: Album): AlbumDraft {
  return {
    name: album.name,
    description: album.description || '',
    displayCreatedDate: album.displayCreatedDate || null,
    photoDateStart: album.photoDateStart || null,
    photoDateEnd: album.photoDateEnd || null,
  }
}

/** 返回错误文案，或 null 表示通过。文案与现网逐字一致。 */
export function validateAlbumDraft(draft: AlbumDraft): string | null {
  if (!draft.name.trim() || Array.from(draft.name.trim()).length > 100) {
    return '名称需为 1–100 个字符'
  }
  if (Array.from(draft.description).length > 1000) return '简介不能超过 1000 个字符'
  if (Boolean(draft.photoDateStart) !== Boolean(draft.photoDateEnd)) {
    return '请同时填写图片起止日期，或同时留空使用自动日期'
  }
  if (draft.photoDateStart && draft.photoDateEnd && draft.photoDateStart > draft.photoDateEnd) {
    return '图片开始日期不能晚于结束日期'
  }
  return null
}

/**
 * 对"可见集合"做并集/差集，不改动可见集合之外的 id。
 * 这正是跨页保留选择的机制：全选/取消只传当前页的 48 条。
 */
export function toggleVisibleSelection(
  selected: string[],
  visible: string[],
  checked: boolean,
): string[] {
  const ids = new Set(selected)
  for (const id of visible) {
    if (checked) ids.add(id)
    else ids.delete(id)
  }
  return [...ids]
}

/** 上传允许的扩展名。注意 `jepg` 是现网就有的拼写，动它会改变用户可上传的文件范围。 */
export const ALBUM_UPLOAD_ACCEPT = '.png,.jpg,.jpeg,.jepg,.webp'
const ALBUM_UPLOAD_PATTERN = /\.(png|jpe?g|jepg|webp)$/i

export const isAlbumUploadable = (fileName: string): boolean =>
  ALBUM_UPLOAD_PATTERN.test(fileName)

/** 封面单独上传只接受这三种，比相册上传少一个 `jepg`——两处口径本来就不一致。 */
const COVER_PATTERN = /\.(png|jpe?g|webp)$/i
export const isCoverUploadable = (fileName: string): boolean => COVER_PATTERN.test(fileName)
