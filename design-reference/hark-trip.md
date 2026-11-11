# Trip components

Components for a page that plans a trip: flights, hotels and the days. A page
uses the parts its trip has, since not every trip has flights or hotels. `hark-trip.css` styles
them and is already in the site's folder. A page that uses either links it
after `hark.css`:

```html
<link rel="stylesheet" href="hark.css" />
<link rel="stylesheet" href="hark-trip.css" />
```

## flights

The flights as the library's table, one row per flight, with the airline in
the first column. A round trip is one row with the outbound and the return
as two columns. Give the table the class `flights` and highlight the pick
with `is-highlight`.

Use it for: two to about eight flights to choose between.

Skip it when: the trip has one flight already booked (say it in a line), or
no flights at all.

| part     | markup                                                                 | notes |
| -------- | ---------------------------------------------------------------------- | ----- |
| table    | `<div class="table-wrap"><table class="flights">`                      | required |
| carrier  | `<th scope="row"><span class="carrier">...</span></th>` first in a row | required |
| airline  | `<span class="carrier__code" data-airline="ANA">NH</span>`             | required; the airline's name in `data-airline` and its two-letter code inside. Hark swaps in the airline's logo when it has one, so never add a logo image yourself |
| name     | `<span>ANA 7<small>ANA</small></span>` after the airline               | required; the flight, then the airline |
| tag      | `<span class="chip chip--solid">Our pick</span>` after the airline     | optional; each label, such as Our pick, on at most one flight |
| leg      | `<td><span>11:25 AM to 3:40 PM<sup>+1</sup></span><small>11h 15m · Nonstop · HND</small></td>` | the times in a `<span>`, with `<sup>+1</sup>` when it lands a day later; the duration, the stops and the airport under it |
| price    | `<td class="num">$1,180<small>Economy · 6 left</small></td>`           | required; the fare for the whole party, the cabin under it |
| link     | `<td><a class="btn" data-hark-action="book-nh7">Book with Hark</a> <a class="link" href="..." target="_blank" rel="noopener">ana.co.jp</a></td>` | when there is a link, last; the Hark button when Hark can book the flight, declared in hark.json, then the site's link naming the site |

```html
<div class="table-wrap">
  <table class="flights">
    <thead>
      <tr><th>Flight</th><th>Out Wed Nov 12</th><th>Back Mon Nov 17</th><th>Round trip</th><th></th></tr>
    </thead>
    <tbody>
      <tr class="is-highlight">
        <th scope="row">
          <span class="carrier">
            <span class="carrier__code" data-airline="ANA">NH</span>
            <span>ANA 7<small>ANA</small><span class="chip chip--solid">Our pick</span></span>
          </span>
        </th>
        <td><span>11:25 AM to 3:40 PM<sup>+1</sup></span><small>11h 15m · Nonstop · HND</small></td>
        <td><span>5:05 PM to 10:15 AM</span><small>9h 10m · Nonstop · HND</small></td>
        <td class="num">$1,180<small>Economy · 6 left</small></td>
        <td><a class="btn" data-hark-action="book-nh7">Book with Hark</a> <a class="link" href="https://www.ana.co.jp" target="_blank" rel="noopener">ana.co.jp</a></td>
      </tr>
    </tbody>
  </table>
</div>
```

## hotels

The hotels as the library's photo cards, one per hotel in the order the
request ranks them, with the library's map right before them when the hotels
have addresses. Give each card an `id`, and number the map's pins in the
cards' order, each linking to its card: the page then shows the hotels as a
gallery or as a map, with a switch between the two.

| part      | markup | notes |
| --------- | ------ | ----- |
| map       | the library's map, its pins' `data-address` each hotel's full street address | optional; when two or more hotels have addresses |
| card      | the library's photo card with `id="<hotel>"` | required, one per hotel |
| price     | the stay's total in `photo-card__price` | the whole stay, for the whole party |
| meta      | `<p class="photo-card__meta">Shibuya · Cosy Double, $420 a night</p>` | the area, the room and its nightly price |
| booked    | `<span class="chip chip--solid">Booked</span>` and the confirmation number in the meta | once it is booked |
| hark      | `<a class="btn" data-hark-action="book-trunk">Book with Hark</a>` | when Hark can book the stay, declared in hark.json; before the link |
| link      | `<a class="link" ...>Book on trunk-hotel.com</a>` under a Hark button, else `<a class="photo-card__link" ...>` | when there is a link; name the site |

What decides between the hotels, such as rating, room size, breakfast or the
time to the airport, goes in the library's compare after the cards, not in
the cards.

## day

A day of the trip in the library's timeline: the date, a few words on what
the day is, and an optional photo of the place on the left; its plans down
the right, each with its start time, what it is with its price and length
beside it, a short note, and an optional tag on the right.

Use it for: an itinerary of a few days, with what to do in each.

Skip it when: the plans have no times or days (use cards or a list).

| part   | markup                                                    | notes |
| ------ | --------------------------------------------------------- | ----- |
| list   | `<ol class="timeline">`                                   | required |
| day    | `<li>`                                                    | required, one per day |
| when   | `<div class="day__when">` first in the day                | required |
| date   | `<p class="timeline__when">Fri Nov 14</p>` in the when    | required |
| theme  | `<p class="day__theme">Harajuku and the bay</p>`          | optional; a few words |
| photo  | `<img class="day__photo" src="..." alt="..." loading="lazy">` | optional; the day's main place, credited in the footer |
| plans  | `<div class="timeline__what"><ol class="plans">`          | required |
| plan   | `<li class="plan">`                                       | required, one per plan, in time order |
| note   | `<p class="fineprint">` in the plans' column, before or after the `<ol class="plans">` | optional; a line about the whole day, such as what is closed that day or an alternative plan |
| time   | `<span class="plan__time">8:00 AM</span>`                 | required; when to start, as a clock time, or Morning, Lunch, Evening when there is no set time. Never a range or opening hours: those go in the note |
| what   | `<p><span class="plan__what">Meiji Jingu</span><span class="plan__note">In through the forest from Harajuku station.</span></p>` | required; the name only, with the note optional |
| fact   | `<span class="plan__fact">$28</span>` right after the name, inside the `<p>` | optional, at most two: the price per person (`Free` when free) and how long it takes (`2 h`). The note does not repeat them |
| tag    | `<span class="pill">Timed tickets</span>` last in the plan | optional; only what to do before going, such as `Book ahead`, `Timed tickets` or `Cash only`; `<span class="chip chip--solid">Booked</span>` once it is booked |

```html
<ol class="timeline">
  <li>
    <div class="day__when">
      <p class="timeline__when">Fri Nov 14</p>
      <p class="day__theme">Harajuku and the bay</p>
      <img class="day__photo" src="meiji.jpg" alt="The torii at Meiji Jingu" loading="lazy" />
    </div>
    <div class="timeline__what">
      <ol class="plans">
        <li class="plan">
          <span class="plan__time">8:00 AM</span>
          <p><span class="plan__what">Meiji Jingu, before the crowds</span><span class="plan__fact">Free</span><span class="plan__fact">1 h</span><span class="plan__note">In through the forest from Harajuku station.</span></p>
        </li>
        <li class="plan">
          <span class="plan__time">2:00 PM</span>
          <p><span class="plan__what">teamLab Planets, Toyosu</span><span class="plan__fact">$28</span><span class="plan__fact">2 h</span><span class="plan__note">You wade through some rooms: wear shorts or roll-ups.</span></p>
          <span class="pill">Timed tickets</span>
        </li>
      </ol>
    </div>
  </li>
</ol>
```
