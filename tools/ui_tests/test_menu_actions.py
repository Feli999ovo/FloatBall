from ui_support import check, enable_model


async def test_menu_actions(orb_page):
    pg, shot, errs = orb_page
    await enable_model(pg)
    await pg.evaluate("collapse()"); await pg.wait_for_timeout(300)
    await pg.evaluate("__emit('menu', 'handoff')"); await pg.wait_for_timeout(1000)
    check(await pg.is_visible("#vHo") and not await pg.is_visible("#vChat"), "右键「交给终端」：从收起状态展开后显示交接面板")
    await pg.evaluate("collapse()"); await pg.wait_for_timeout(300)
    await pg.evaluate("__emit('menu', 'routines')"); await pg.wait_for_timeout(1000)
    g = await pg.evaluate("(() => { const h = document.querySelector('#routineHead').getBoundingClientRect(), t = document.querySelector('#tasks').getBoundingClientRect(); return [h.top, h.bottom, t.top, t.bottom]; })()")
    check(await pg.is_visible("#vTask") and await pg.inner_text("#routineHead") == "定时任务" and g[0] >= g[2] - 1 and g[1] <= g[3] + 1, f"右键「定时任务」：任务页展开，组标题在可见区域 {g}")
    await shot("14b_ctx_routines")
    await pg.evaluate("__emit('menu', 'sleep')"); await pg.wait_for_timeout(1000)
    check(await pg.evaluate("document.body.classList.contains('collapsed') && document.querySelector('#orb').classList.contains('s-sleep')"), "右键「睡眠」：先收起卡片，收起动画结束后仍是睡眠表情")
    await shot("14c_ctx_sleep")
    await pg.evaluate("__emit('menu', 'sleep')"); await pg.wait_for_timeout(100)
    check(await pg.evaluate("document.body.classList.contains('collapsed') && document.querySelector('#orb').classList.contains('s-idle')"), "睡着时再点「睡眠」：唤醒但不展开")
    await pg.evaluate("__emit('menu', 'sleep')"); await pg.wait_for_timeout(100)
    await pg.click("#orb"); await pg.wait_for_timeout(1000)
    check(await pg.is_visible("#vChat") and not await pg.evaluate("document.querySelector('#orb').classList.contains('s-sleep')"), "点睡着的悬浮球：醒来并展开对话")
    await pg.mouse.move(0, 0)  # 光标移出球，避免收起后既有的 hover 唤醒干扰睡眠检查
    await pg.evaluate("__mock.slow = true; __mock.cancelled = null")
    await pg.fill("#ask", "睡眠时后台继续工作"); await pg.press("#ask", "Enter"); await pg.wait_for_timeout(300)
    await pg.evaluate("__emit('menu', 'new_chat')"); await pg.wait_for_timeout(100)
    await pg.evaluate("__emit('menu', 'sleep')"); await pg.wait_for_timeout(1000)
    check(await pg.evaluate("document.querySelector('#orb').classList.contains('s-sleep') && document.querySelector('#tBadge').textContent === '1' && !__mock.cancelled"), "后台任务在跑时也能睡眠，不会停止任务")
    await pg.evaluate("__mock.slow = false")
    await pg.wait_for_function("document.querySelector('#tBadge').hidden", timeout=15000)
    await pg.wait_for_selector("#peek.on", timeout=3000)  # 完成角标先更新，小泡还要等窗口动画
    check(await pg.evaluate("document.querySelector('#orb').classList.contains('s-done') && +document.querySelector('#orb').dataset.done > 0 && document.querySelector('#peek').classList.contains('on')"), "后台完成：唤醒表情，完成小泡和角标照常显示")
    check(not errs, "右键新增入口无 JS 错误 " + "; ".join(errs))
