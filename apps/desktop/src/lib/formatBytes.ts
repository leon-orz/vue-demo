export function formatBytes(bytes: bigint): string {
  if (bytes < 1024n) return `${bytes} B`

  const units = ['KB', 'MB', 'GB', 'TB']
  let value = Number(bytes)
  let unit = -1
  do {
    value /= 1024
    unit += 1
  } while (value >= 1024 && unit < units.length - 1)

  return `${value.toFixed(1)} ${units[unit]}`
}
