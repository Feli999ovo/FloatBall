/* Page behavior for Hark's map: the drawn map put inline so its colors follow the theme, pin labels kept from covering each other, and a map whose pins link to photo cards turned into the cards' map view. style-gates: data */
;(function () {
  'use strict'

  var smooth = matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth'
  var all = function (root, sel) {
    return [].slice.call(root.querySelectorAll(sel))
  }

  // The map ships as an image; read inline, the page's colors reach its streets and water.
  function inline(img) {
    fetch(img.src)
      .then(function (r) {
        return r.ok ? r.text() : ''
      })
      .then(function (text) {
        var doc = new DOMParser().parseFromString(text, 'image/svg+xml')
        var svg = doc.documentElement
        if (!svg || svg.nodeName.toLowerCase() !== 'svg') return
        svg.setAttribute('class', 'map__img')
        svg.setAttribute('aria-hidden', 'true')
        img.replaceWith(document.importNode(svg, true))
      })
      .catch(function (err) {
        console.warn('hark-map: the map stays an image', err)
      })
  }

  function overlaps(a, b) {
    return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
  }

  // Each label sits under its pin; one that would cover a pin or an earlier label, or leave the
  // visible map, moves above its pin, and one that still would is hidden, leaving the pin's number.
  function placeLabels(map) {
    var pins = all(map, '.map__pin:not([hidden])')
    // An inset keeps a lit pin's ring, drawn outside its bubble, on the map too.
    var edge = (map.querySelector('.map__stage') || map).getBoundingClientRect()
    var box = {
      left: edge.left + 8,
      right: edge.right - 8,
      top: edge.top + 8,
      bottom: edge.bottom - 8,
    }
    separatePins(pins, box)
    var taken = pins.map(function (pin) {
      return pin.getBoundingClientRect()
    })
    var credit = map.querySelector('.map__credit')
    if (credit) taken.push(credit.getBoundingClientRect())
    pins.forEach(function (pin) {
      var label = pin.querySelector('.map__label')
      if (!label) return
      pin.classList.remove('is-label-above', 'is-label-quiet')
      var fits = function () {
        var r = label.getBoundingClientRect()
        return (
          inside(r, box) &&
          !taken.some(function (t) {
            return overlaps(r, t)
          })
        )
      }
      if (!fits()) pin.classList.add('is-label-above')
      if (!fits()) {
        pin.classList.remove('is-label-above')
        pin.classList.add('is-label-quiet')
        return
      }
      taken.push(label.getBoundingClientRect())
    })
  }

  // inside reports whether rect lies within box.
  function inside(rect, box) {
    return (
      rect.left >= box.left &&
      rect.right <= box.right &&
      rect.top >= box.top &&
      rect.bottom <= box.bottom
    )
  }

  // Pins for places a block apart would overlap, so a pin that would touch an earlier one moves to sit
  // beside it: away from it, then to either side, then past it, taking the first spot that is clear
  // and on the visible map. With no such spot it stays where its place is.
  function separatePins(pins, box) {
    var placed = []
    pins.forEach(function (pin) {
      pin.style.setProperty('--nx', '0px')
      pin.style.setProperty('--ny', '0px')
      var r = pin.getBoundingClientRect()
      var hit = placed.filter(function (p) {
        return overlaps(r, p)
      })[0]
      if (hit) {
        var cx = (r.left + r.right) / 2
        var cy = (r.top + r.bottom) / 2
        var hx = (hit.left + hit.right) / 2
        var hy = (hit.top + hit.bottom) / 2
        var len = Math.hypot(cx - hx, cy - hy) || 1
        var ux = (cx - hx) / len || 1
        var uy = (cy - hy) / len
        var dirs = [
          [ux, uy],
          [-uy, ux],
          [uy, -ux],
          [-ux, -uy],
        ]
        for (var i = 0; i < dirs.length; i++) {
          // Far enough along dirs[i] that the two boxes clear on the axis it mostly runs along.
          var reach = Math.min(
            Math.abs(dirs[i][0]) > 0.01
              ? ((r.width + hit.right - hit.left) / 2 + 4) / Math.abs(dirs[i][0])
              : Infinity,
            Math.abs(dirs[i][1]) > 0.01
              ? ((r.height + hit.bottom - hit.top) / 2 + 4) / Math.abs(dirs[i][1])
              : Infinity,
          )
          var dx = hx + dirs[i][0] * reach - cx
          var dy = hy + dirs[i][1] * reach - cy
          var moved = {
            left: r.left + dx,
            right: r.right + dx,
            top: r.top + dy,
            bottom: r.bottom + dy,
          }
          var clear = !placed.some(function (p) {
            return overlaps(moved, p)
          })
          if (clear && inside(moved, box)) {
            pin.style.setProperty('--nx', dx + 'px')
            pin.style.setProperty('--ny', dy + 'px')
            r = pin.getBoundingClientRect()
            break
          }
        }
      }
      placed.push(r)
    })
  }

  // The photo card each pin links to, or null when any pin links to none.
  function linkedCards(map) {
    var cards = all(map, '.map__pin').map(function (pin) {
      var card = document.getElementById((pin.getAttribute('href') || '').slice(1))
      return card && card.classList.contains('photo-card') ? card : null
    })
    return cards.length > 1 && cards.every(Boolean) ? cards : null
  }

  function text(card, sel) {
    var el = card.querySelector(sel)
    return el ? el.textContent.trim() : ''
  }

  // A compact copy of a photo card for the strip over the map: its first photo, rank, name, meta,
  // price and actions.
  function stripCard(card) {
    var item = document.createElement('article')
    item.className = 'map__card'
    item.dataset.key = card.id
    var photo = card.querySelector('.photo-card__photos img')
    if (photo) {
      var img = document.createElement('img')
      img.src = photo.currentSrc || photo.src
      img.alt = ''
      img.className = 'map__card-photo'
      item.appendChild(img)
    }
    var body = document.createElement('div')
    body.className = 'map__card-body'
    ;[
      ['map__card-rank', text(card, '.photo-card__rank')],
      ['map__card-name', text(card, 'h3')],
      ['map__card-meta', text(card, '.photo-card__meta')],
    ].forEach(function (part) {
      if (!part[1]) return
      var p = document.createElement('p')
      p.className = part[0]
      p.textContent = part[1]
      body.appendChild(p)
    })
    var price = card.querySelector('.photo-card__price')
    if (price && price.textContent.trim()) body.appendChild(copyPrice(price))
    body.appendChild(stripActions(card))
    item.appendChild(body)
    return item
  }

  // The price keeps its markup, so an old price the card strikes out stays struck out.
  function copyPrice(price) {
    var p = document.createElement('p')
    p.className = 'map__card-price'
    ;[].forEach.call(price.childNodes, function (node) {
      p.appendChild(node.cloneNode(true))
    })
    return p
  }

  // Smaller copies of a card's actions: its Hark button, which Hark reveals only to the page's
  // owner, and its store or booking link, which is the card body's last link.
  function stripActions(card) {
    var row = document.createElement('div')
    row.className = 'map__card-actions'
    var ask = card.querySelector('[data-hark-action]')
    if (ask) {
      var b = document.createElement('a')
      b.className = 'map__card-link'
      b.setAttribute('data-hark-action', ask.getAttribute('data-hark-action'))
      b.textContent = ask.textContent.trim()
      row.appendChild(b)
    }
    var site = all(card, '.photo-card__body > a[href]').pop()
    if (site) {
      var a = document.createElement('a')
      a.className = ask ? 'link map__card-site' : 'map__card-link'
      a.href = site.href
      a.textContent = site.textContent.trim()
      if (site.target) a.target = site.target
      if (site.rel) a.rel = site.rel
      row.appendChild(a)
    }
    return row
  }

  // The Gallery and Map switch above the cards.
  function viewSwitch(show) {
    var bar = document.createElement('div')
    bar.className = 'view-switch'
    bar.setAttribute('role', 'radiogroup')
    bar.setAttribute('aria-label', 'Show as')
    ;['Gallery', 'Map'].forEach(function (name, i) {
      var b = document.createElement('button')
      b.type = 'button'
      b.textContent = name
      b.setAttribute('role', 'radio')
      b.setAttribute('aria-checked', String(i === 0))
      b.addEventListener('click', function () {
        all(bar, 'button').forEach(function (o) {
          o.setAttribute('aria-checked', String(o === b))
        })
        show(name === 'Map')
      })
      bar.appendChild(b)
    })
    return bar
  }

  // The share of a map view's height kept clear at its foot for the strip: cardStripFrame in
  // internal/tools/website/site_lookup_map.go.
  var STRIP_SHARE = 0.3

  // The strip lies over the map's foot only when its tallest card fits the band the map keeps
  // clear. Otherwise it runs under the map, as on a phone, so no card is cut short.
  function fitStrip(map, strip) {
    map.classList.remove('map--below')
    var pad = getComputedStyle(strip)
    var room =
      map.clientHeight * STRIP_SHARE - parseFloat(pad.paddingTop) - parseFloat(pad.paddingBottom)
    map.classList.add('map--below')
    var tallest = Math.max.apply(
      null,
      all(strip, '.map__card').map(function (card) {
        return card.offsetHeight
      }),
    )
    map.classList.toggle('map--below', tallest > room)
  }

  // A map whose pins link to photo cards becomes their map view: a switch shows the cards or the
  // map, and on the map a strip of the cards runs along its foot. Swiping the strip lights the
  // middle card's pin; a pin or a card brings that card to the middle.
  function mapView(map, cards) {
    var row = cards[0].closest('.photo-cards')
    if (!row) return
    var pins = all(map, '.map__pin')
    var strip = document.createElement('div')
    strip.className = 'map__strip'
    var items = cards.map(function (card) {
      return strip.appendChild(stripCard(card))
    })
    // The map and its pins move into a stage of the map's own shape, so a strip laid under it on a
    // phone leaves the pins where the map put them.
    var stage = document.createElement('div')
    stage.className = 'map__stage'
    var frame = stage.appendChild(document.createElement('div'))
    frame.className = 'map__frame'
    ;[].slice.call(map.childNodes).forEach(function (node) {
      frame.appendChild(node)
    })
    map.appendChild(stage)
    map.classList.add('map--view')
    map.appendChild(strip)

    var light = function (key) {
      pins.forEach(function (pin, i) {
        pin.classList.toggle('is-lit', cards[i].id === key)
      })
      items.forEach(function (item) {
        item.classList.toggle('is-lit', item.dataset.key === key)
      })
    }
    // While a tap scrolls the strip to its card, the cards it passes are not lit along the way.
    var steering = false
    var release
    var stopSteering = function () {
      steering = false
      clearTimeout(release)
    }
    strip.addEventListener('scrollend', stopSteering)
    var centre = function (key) {
      var item = items.filter(function (it) {
        return it.dataset.key === key
      })[0]
      light(key)
      if (!item) return
      steering = true
      clearTimeout(release)
      release = setTimeout(stopSteering, 1000)
      strip.scrollTo({
        left: item.offsetLeft - (strip.clientWidth - item.offsetWidth) / 2,
        behavior: smooth,
      })
    }
    pins.forEach(function (pin, i) {
      pin.addEventListener('click', function (ev) {
        ev.preventDefault()
        centre(cards[i].id)
      })
    })
    items.forEach(function (item) {
      item.addEventListener('click', function (ev) {
        if (!ev.target.closest('a')) centre(item.dataset.key)
      })
    })
    var ticking = false
    strip.addEventListener(
      'scroll',
      function () {
        if (ticking || steering) return
        ticking = true
        requestAnimationFrame(function () {
          ticking = false
          var mid = strip.scrollLeft + strip.clientWidth / 2
          var near = items.reduce(function (a, b) {
            return Math.abs(b.offsetLeft + b.offsetWidth / 2 - mid) <
              Math.abs(a.offsetLeft + a.offsetWidth / 2 - mid)
              ? b
              : a
          })
          // At either end of the strip its end card is the one meant, wherever the middle falls.
          if (strip.scrollLeft <= 1) near = items[0]
          else if (strip.scrollLeft + strip.clientWidth >= strip.scrollWidth - 1)
            near = items[items.length - 1]
          light(near.dataset.key)
        })
      },
      { passive: true },
    )

    var show = function (onMap) {
      map.hidden = !onMap
      row.hidden = onMap
      if (onMap) {
        fitStrip(map, strip)
        light(items[0].dataset.key)
        placeLabels(map)
      }
    }
    window.addEventListener('resize', function () {
      if (map.hidden) return
      fitStrip(map, strip)
      placeLabels(map)
    })
    // A web font that arrives late changes the cards' height, so the strip is fitted again.
    if (document.fonts)
      document.fonts.ready.then(function () {
        if (!map.hidden) fitStrip(map, strip)
      })
    var first = map.compareDocumentPosition(row) & Node.DOCUMENT_POSITION_FOLLOWING ? map : row
    first.parentNode.insertBefore(viewSwitch(show), first)
    show(false)
  }

  function init() {
    all(document, '.map[data-map-drawn]').forEach(function (map) {
      var img = map.querySelector('img.map__img')
      if (img) inline(img)
      var cards = linkedCards(map)
      if (cards) {
        mapView(map, cards)
        return
      }
      placeLabels(map)
      window.addEventListener('resize', function () {
        placeLabels(map)
      })
    })
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init)
  else init()
})()
