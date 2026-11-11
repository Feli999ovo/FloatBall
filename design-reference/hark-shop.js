/* Page behavior for Hark's shop cards: a tap that turns a product with several photos to the next one. style-gates: data */
;(function () {
  'use strict'
  function all(root, sel) {
    return [].slice.call(root.querySelectorAll(sel))
  }

  // A product with one photo stays still; with several, a tap or Enter turns it to the next photo.
  function wireTurn(photos) {
    var shots = all(photos, 'img')
    if (shots.length < 2) return
    var name = photos.closest('.product')
    var title = name && name.querySelector('h3') ? name.querySelector('h3').textContent.trim() : ''
    var pips = document.createElement('div')
    pips.className = 'product__angles'
    pips.setAttribute('aria-hidden', 'true')
    shots.forEach(function () {
      pips.appendChild(document.createElement('i'))
    })
    photos.parentNode.insertBefore(pips, photos.nextSibling)
    var shown = 0
    function show(at) {
      shown = at
      shots.forEach(function (img, i) {
        img.classList.toggle('is-on', i === at)
      })
      all(pips, 'i').forEach(function (p, i) {
        p.classList.toggle('is-on', i === at)
      })
      photos.setAttribute(
        'aria-label',
        'Turn ' + (title || 'the product') + ', photo ' + (at + 1) + ' of ' + shots.length,
      )
    }
    photos.setAttribute('data-turn', '')
    photos.setAttribute('role', 'button')
    photos.tabIndex = 0
    photos.addEventListener('click', function () {
      show((shown + 1) % shots.length)
    })
    photos.addEventListener('keydown', function (ev) {
      if (ev.key !== 'Enter' && ev.key !== ' ') return
      ev.preventDefault()
      show((shown + 1) % shots.length)
    })
    show(0)
  }

  function init() {
    all(document, '.product__photos').forEach(wireTurn)
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init)
  else init()
})()
