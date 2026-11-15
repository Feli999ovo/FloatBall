import asyncio, json, os, sys
os.environ.setdefault('PLAYWRIGHT_BROWSERS_PATH','/workspace/work/shot/pw')
from playwright.async_api import async_playwright
PROTO="/workspace/built/sites/2b587eb3-3ea1-4b88-971d-2da7e5f82670/index.html"
SRC=open('/workspace/work/fb/floatball/tools/extract_proto_settings_css.py').read()
JS=SRC.split('JS = r"""')[1].split('"""')[0].replace("const out = [];","const out = []; let n = 0;").replace("for (const r of rules) {","for (const r of rules) { n++;").replace("out.push(r.cssText)","out.push([n, r.cssText])")
# collect rules per state, union preserving stylesheet order -> use index
JS2 = JS.replace("out.push(r.cssText)","out.push(r.cssText)")
async def main():
    async with async_playwright() as p:
        b = await p.chromium.launch()
        pg = await b.new_page(viewport={"width":1440,"height":900}, device_scale_factor=2)
        await pg.goto("file://"+PROTO); await pg.wait_for_timeout(1500)
        await pg.evaluate("document.getElementById('setBtn').click()"); await pg.wait_for_timeout(500)
        r = await pg.evaluate("(()=>{const r=document.querySelector('.x-setw').getBoundingClientRect();return [r.x,r.y,r.width,r.height]})()")
        clip={"x":r[0]-20,"y":r[1]-20,"width":r[2]+40,"height":r[3]+40}
        allr=[]
        async def grab(name):
            await pg.wait_for_timeout(400)
            allr.extend(await pg.evaluate(JS))
            await pg.screenshot(path=f'p_{name}.png', clip=clip)
        tab=lambda t: pg.evaluate(f"document.querySelector('.x-tabs [data-tab={t}]').click()")
        await grab('lib')
        await pg.evaluate("document.getElementById('libAdd').click()"); await grab('add')
        await pg.fill('#fModel','gpt-4o'); await pg.evaluate("document.getElementById('fTest').click()"); await pg.wait_for_timeout(1200)
        await pg.evaluate("document.getElementById('fFetch').click()"); await pg.wait_for_timeout(900)
        await pg.evaluate("""(()=>{const w=document.querySelector('.x-setw .x-wf');const a=document.createElement('span');a.className='x-res bad';const b=document.createElement('span');b.className='x-spin';w.append(a,b);})()""")
        await grab('add_list')
        await pg.evaluate("document.querySelectorAll('.x-wf > .x-res.bad, .x-wf > .x-spin').forEach(e=>e.remove()); document.getElementById('fBack').click(); document.getElementById('fList').hidden=true")
        await tab('agents'); await grab('agents')
        await pg.evaluate("document.getElementById('agAdd').click()"); await grab('agents_tpl')
        await pg.evaluate("document.querySelector('#agList .x-agc button').click()"); await grab('aged')
        await pg.evaluate("document.getElementById('paneAgEd').scrollTop=9999"); await grab('aged_bottom')
        await pg.fill('#aeTry','帮我检查这份报告的错别字'); await grab('aged_try')
        await tab('perm'); await grab('perm')
        await tab('look'); await grab('look')
        await pg.evaluate("document.getElementById('paneLook').scrollTop=9999"); await grab('look_bottom')
        await tab('keys'); await grab('keys')
        await tab('sel'); await grab('sel')
        await pg.evaluate("document.getElementById('paneSel').scrollTop=9999"); await grab('sel_bottom')
        await tab('voice'); await grab('voice')
        d={}
        for n,t in allr: d[n]=t
        out=[d[k] for k in sorted(d)]
        json.dump(out,open("rules_all.json","w"),ensure_ascii=False); print(len(out))
        await b.close()
asyncio.run(main())
