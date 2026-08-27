/** Animation completion can stall in an occluded/background browser tab. */
export function animationDeadline(finished: Promise<unknown>, timeoutMs = 320): Promise<void> {
  return new Promise(resolve => {
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      clearTimeout(timer)
      resolve()
    }
    const timer = setTimeout(finish, timeoutMs)
    void finished.then(finish, finish)
  })
}
