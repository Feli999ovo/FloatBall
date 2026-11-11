# Shop components

Components for a page that helps someone choose something to buy: a plain
title, a shelf of product cards, and the library's compare with its photos on
the same stage. `hark-shop.css` styles them and `hark-shop.js` turns a
product with several photos; both are already in the site's folder. A page
that uses any of them links the stylesheet after `hark.css` and loads the
script after `hark.js`:

```html
<link rel="stylesheet" href="hark.css" />
<link rel="stylesheet" href="hark-shop.css" />
...
<script src="hark.js"></script>
<script src="hark-shop.js"></script>
```

Each block of the page after the shelf is the library's `section` with its
`head`; on a shop page the head stays left-aligned.

## shop title

The page's title with no hero, left-aligned on the page.

| part    | markup                               | notes                                                         |
| ------- | ------------------------------------ | ------------------------------------------------------------- |
| wrapper | `<header class="shop-title">`        | required, first in `main`                                     |
| title   | `<h1>`                               | required; what is being bought, in plain words                |
| eyebrow | `<p class="shop-title__eyebrow">`    | optional; a short line under the title, such as when prices were checked |
| intro   | `<p class="shop-title__intro">`      | optional; the answer in two sentences at most, linking to the card it names with `link` |

```html
<header class="shop-title">
  <h1>The five best-selling Rimowa carry-ons</h1>
  <p class="shop-title__eyebrow">US prices as of Oct 23, 2026</p>
  <p class="shop-title__intro">Our pick is the <a class="link" href="#classic">Classic Cabin</a>. The <a class="link" href="#essential">Essential Cabin</a> is the value.</p>
</header>
```

## shelf

The products as tall cards in the order the request ranks them, wrapping into
rows: three to a row on a desktop, two on a tablet, one on a phone.

Use it for: the products the page compares, up to about eight, right after
the title.

Skip it when: there are many more products than a reader would scan as cards
(use a table for the long tail).

| part    | markup                                                         | notes |
| ------- | -------------------------------------------------------------- | ----- |
| shelf   | `<section class="shelf" id="products" data-nav="Products">`    | required; not a `section`, so it has no `head` |
| label   | `<div class="shelf__head"><p class="shelf__label">...</p></div>` | required; one line on the order |
| row     | `<div class="deck">`                                           | required |
| card    | `<article class="product" id="...">`                           | required, one per product |
| stage   | `<div class="product__stage">` first in the card               | required |
| rank    | `<p class="rank">Our pick</p>` in the stage                    | optional; why it is on the page: _Our pick_, _Cheapest_, _Biggest_ |
| photos  | `<div class="product__photos">` of `<img>` in the stage        | required when there is a photo; up to three angles of the same product |
| body    | `<div class="product__body">`                                  | required |
| head    | `<div class="product__head"><h3>Name</h3><p class="product__price">$990</p></div>` | required; `<s>` before the price for a real list price |
| meta    | `<p class="product__meta">`                                    | optional; material, size or edition, one short line |
| note    | `<p class="product__note">`                                    | optional; one or two sentences of judgement |
| warning | `<p class="dot dot--no">`                                      | optional; one thing that rules it out for some readers, such as "Too big for most US carry-on limits" |
| colors  | `<p class="product__colors"><span class="minis" aria-hidden="true"><i style="--swatch:#b4b4b4" title="Silver"></i></span>2 colors</p>` | optional; one dot per color, then the count |
| buy     | `<a class="buy" href="..." target="_blank" rel="noopener">Shop on rimowa.com</a>` | required when there is a link, last in the body; name the store |

Every card keeps the same parts in the same order, so the cards line up. Every
photo is a file or URL in the markup itself; give every photo after a card's
first `loading="lazy"`.

```html
<section class="shelf" id="products" data-nav="The five">
  <div class="shelf__head">
    <p class="shelf__label">In Rimowa's best-seller order.</p>
  </div>
  <div class="deck">
    <article class="product" id="classic">
      <div class="product__stage">
        <p class="rank">Our pick</p>
        <div class="product__photos">
          <img src="classic-front.webp" alt="Classic Cabin in silver, front" />
          <img src="classic-side.webp" alt="Classic Cabin in silver, side" loading="lazy" />
          <img src="classic-back.webp" alt="Classic Cabin in silver, back" loading="lazy" />
        </div>
      </div>
      <div class="product__body">
        <div class="product__head"><h3>Classic Cabin</h3><p class="product__price">$1,650</p></div>
        <p class="product__meta">Anodized aluminum · latches</p>
        <p class="product__note">The heritage icon in grooved metal. The heaviest here, and it dents into a patina.</p>
        <p class="product__colors"><span class="minis" aria-hidden="true"><i style="--swatch:#b4b4b4" title="Silver"></i><i style="--swatch:#877e6c" title="Titanium"></i></span>2 colors</p>
        <a class="buy" href="https://www.rimowa.com" target="_blank" rel="noopener">Shop on rimowa.com</a>
      </div>
    </article>
  </div>
</section>
```

## compare

The products side by side use the library's compare, as it shows, with each
option's photo, a chip for its rank label, and the specs that decide the
choice; bold the best value in a row with `<strong>`. On a shop page its photos
sit on the same stage as the cards. A number worth reading at a glance, such
as weight or volume, can carry `<span class="meter" style="--v:0.71"></span>`
after it: a bar on one scale for its whole row, from zero, where `--v` is the
value over the row's scale. A yes or no row reads as `<span class="dot">Fits
most</span>` or `<span class="dot dot--no">Too big</span>`. Under a count of
colors, `<span class="minis">` shows them as small dots. For more than five
products, use a table instead.
