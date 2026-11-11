/* Page behavior for Hark's photo cards: a tap that shows a card's next photo, and photos that fail to load taken out. style-gates: data */
;(function () {
  'use strict'

  // A card with one photo stays still; with several, a tap, Enter or Space shows the next photo.
  function wire(photos) {
    var shots = [].slice.call(photos.querySelectorAll('img'))
    if (shots.length < 2 || photos.hasAttribute('data-turn')) return
    var card = photos.closest('.photo-card')
    var h3 = card && card.querySelector('h3')
    var title = h3 ? h3.textContent.trim() : 'this item'
    var dots = document.createElement('div')
    dots.className = 'photo-card__dots'
    dots.setAttribute('aria-hidden', 'true')
    shots.forEach(function () {
      dots.appendChild(document.createElement('i'))
    })
    photos.parentNode.insertBefore(dots, photos.nextSibling)
    var shown = 0
    function show(at) {
      shown = at
      shots.forEach(function (img, i) {
        img.classList.toggle('is-on', i === at)
      })
      ;[].forEach.call(dots.children, function (dot, i) {
        dot.classList.toggle('is-on', i === at)
      })
      photos.setAttribute(
        'aria-label',
        'Next photo of ' + title + ', showing ' + (at + 1) + ' of ' + shots.length,
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

  // A photo that fails to load leaves the card. A card left with none loses its photo cell, and
  // hark-cards.css then lays it out as a card without a photo.
  function setUp(photos) {
    ;[].slice.call(photos.querySelectorAll('img')).forEach(function (img) {
      if (img.complete && !img.naturalWidth) img.remove()
      else
        img.addEventListener('error', function () {
          drop(photos, img)
        })
    })
    if (!photos.querySelector('img')) photos.remove()
    else wire(photos)
  }

  // Rebuilds the photos without the failed one: a copy sheds the old tap handlers and dots.
  function drop(photos, img) {
    img.remove()
    var dots = photos.nextElementSibling
    if (dots && dots.classList.contains('photo-card__dots')) dots.remove()
    var fresh = photos.cloneNode(true)
    ;['data-turn', 'role', 'tabindex', 'aria-label'].forEach(function (name) {
      fresh.removeAttribute(name)
    })
    photos.replaceWith(fresh)
    setUp(fresh)
  }

  function init() {
    ;[].slice.call(document.querySelectorAll('.photo-card__photos')).forEach(setUp)
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init)
  else init()
})()
