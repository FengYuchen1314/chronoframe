export interface AlbumDraft {
  name: string
  description: string
  displayCreatedDate: string | null
  photoDateStart: string | null
  photoDateEnd: string | null
}

export function albumDraftOf(album: AlbumDraft): AlbumDraft {
  return {
    name: album.name, description: album.description || '',
    displayCreatedDate: album.displayCreatedDate || null,
    photoDateStart: album.photoDateStart || null, photoDateEnd: album.photoDateEnd || null,
  }
}

export function validateAlbumDraft(draft: AlbumDraft): string | null {
  if (!draft.name.trim() || Array.from(draft.name.trim()).length > 100) return '名称需为 1–100 个字符'
  if (Array.from(draft.description).length > 1000) return '简介不能超过 1000 个字符'
  if (!!draft.photoDateStart !== !!draft.photoDateEnd) return '请同时填写图片起止日期，或同时留空使用自动日期'
  if (draft.photoDateStart && draft.photoDateEnd && draft.photoDateStart > draft.photoDateEnd) return '图片开始日期不能晚于结束日期'
  return null
}

export function toggleVisibleSelection(selected: string[], visible: string[], checked: boolean): string[] {
  const ids = new Set(selected)
  for (const id of visible) { if (checked) ids.add(id); else ids.delete(id) }
  return [...ids]
}
