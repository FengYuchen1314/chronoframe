export const isAbortedRequest = (error: unknown, signal: AbortSignal) => {
  if (signal.aborted) return true

  let current = error
  for (let depth = 0; depth < 3 && current && typeof current === 'object'; depth += 1) {
    const candidate = current as { name?: unknown; code?: unknown; cause?: unknown }
    if (candidate.name === 'AbortError' || candidate.code === 'ABORT_ERR') return true
    current = candidate.cause
  }

  return false
}
