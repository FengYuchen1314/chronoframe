// The cover is presentation only: never reorder the album's actual photo collection.
export function albumCoverStack(album: { coverUrl: string | null; photos: { id: string; thumbnailUrl: string }[] }) {
  const photos = album.photos
  if (!album.coverUrl) return photos.slice(0, 3)
  return [
    { id: 'cover', thumbnailUrl: album.coverUrl },
    ...photos.filter(photo => photo.thumbnailUrl !== album.coverUrl).slice(0, 2),
  ]
}
