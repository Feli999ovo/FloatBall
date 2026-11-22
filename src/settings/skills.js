import { mk } from "./agent-form.js";
import { reload } from "./startup.js";
import { $, invoke, mutable, toast } from "./state.js";

export let editingSkill, skills;

export function editSkill(skill) {
  editingSkill = skill;
  $("skillEntries").hidden = true;
  $("skillEditor").hidden = false;
  $("libSkillName").value = skill.name;
  $("libSkillWhen").value = skill.when;
  $("libSkillSteps").value = skill.steps;
  $("libSkillName").focus();
}

export function closeSkillEditor() {
  editingSkill = null;
  $("skillEntries").hidden = false;
  $("skillEditor").hidden = true;
}

export function renderSkillLibrary() {
  const list = $("skillEntries");
  list.replaceChildren();
  if (!skills().length) {
    list.append(mk("p", "x-note", "还没有技能。可从对话回复存入。"));
    return;
  }
  for (const skill of skills()) {
    const item = mk("details");
    const title = mk("summary", "", skill.name);
    const when = mk("p", "x-note", skill.when);
    const steps = mk("p", "skill-steps", skill.steps);
    const edit = mk("button", "x-sb", "编辑");
    const remove = mk("button", "x-sb", "删除");
    remove.type = edit.type = "button";
    edit.onclick = () => editSkill(skill);
    remove.onclick = async () => {
      if (remove.dataset.confirm !== "true") {
        remove.dataset.confirm = "true";
        remove.textContent = "确认删除";
        return;
      }
      remove.disabled = true;
      try {
        await invoke("delete_skill", { id: skill.id });
        await reload();
        toast("已删除技能");
      } catch (err) {
        toast(String(err));
        remove.disabled = false;
      }
    };
    item.append(title, when, steps, edit, remove);
    list.append(item);
  }
}

export function renderAgentSkills(group, draft) {
  group.replaceChildren();
  if (!skills().length) group.append(mk("span", "x-aelk", "技能库里还没有技能"));
  for (const skill of skills()) {
    const button = mk("button", "x-lbtl", skill.name);
    button.type = "button";
    button.setAttribute("aria-pressed", draft.skills.includes(skill.id));
    button.onclick = () => {
      const selected = !draft.skills.includes(skill.id);
      if (selected) draft.skills.push(skill.id);
      else draft.skills = draft.skills.filter((id) => id !== skill.id);
      button.setAttribute("aria-pressed", selected);
    };
    group.append(button);
  }
}

export function initSkills() {
  mutable.agEd = null;
  editingSkill = null;
  skills = () => mutable.cfg?.skills || [];
  $("libSkillCancel").onclick = closeSkillEditor;
  $("skillEditor").onsubmit = async (e) => {
    e.preventDefault();
    const editing = editingSkill,
      save = $("libSkillSave");
    if (!editing || save.disabled) return;
    const skill = {
      id: editing.id,
      name: $("libSkillName").value,
      when: $("libSkillWhen").value,
      steps: $("libSkillSteps").value,
    };
    save.disabled = true;
    try {
      await invoke("save_skill", { skill });
      if (editingSkill === editing) closeSkillEditor();
      await reload();
      toast("已保存技能");
    } catch (err) {
      toast(String(err));
    } finally {
      save.disabled = false;
    }
  };
}
