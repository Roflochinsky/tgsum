export function mountBackground({ invoke, toast }) {
  const dialog = document.querySelector('#background-dialog')
  const checkbox = dialog.querySelector('[data-background-autostart]')
  const status = dialog.querySelector('[data-background-status]')
  const error = dialog.querySelector('[data-background-error]')
  const path = dialog.querySelector('[data-background-path]')
  const hide = dialog.querySelector('[data-background-hide]')
  let pending = false
  let generation = 0

  function render(view) {
    checkbox.checked = view.autostart_enabled
    checkbox.disabled = pending || !view.available
    hide.disabled = !view.available
    status.textContent = !view.available ? 'Работа в фоне пока доступна только на Linux.'
      : view.keeps_running ? 'Закрытие окна оставит TGSUM работать в фоне.'
        : 'Сейчас включённого сбора нет. Можно оставить приложение в фоне кнопкой ниже.'
    path.textContent = view.service_path ? `Настройка автозапуска: ${view.service_path}` : ''
  }

  function failed(e) {
    error.textContent = e?.message || String(e)
    error.hidden = false
  }

  async function load() {
    const request = ++generation
    try {
      const view = await invoke('background_status')
      if (request === generation) render(view)
    } catch (e) { if (request === generation) { checkbox.disabled = true; failed(e) } }
  }

  document.querySelector('#btn-background').addEventListener('click', () => {
    error.hidden = true
    dialog.showModal()
    load()
  })
  window.addEventListener('focus', () => { if (dialog.open && !pending) load() })

  checkbox.addEventListener('change', async () => {
    if (pending) return
    const enabled = checkbox.checked
    ++generation
    pending = true
    checkbox.disabled = true
    error.hidden = true
    try {
      render(await invoke('set_background_autostart', { enabled }))
      toast(enabled ? 'Автозапуск включён для следующего входа в систему.' : 'Автозапуск выключен. Текущая работа продолжается.')
    } catch (e) { failed(e) }
    finally { pending = false; await load() }
  })

  hide.addEventListener('click', async () => {
    try { await invoke('hide_application'); dialog.close() }
    catch (e) { failed(e) }
  })
  dialog.querySelector('[data-background-quit]').addEventListener('click', () => {
    invoke('quit_application').catch(failed)
  })
}
