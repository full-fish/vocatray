// 카드와 퀴즈 창이 같이 쓰는 단어 그리기
export type Sense = { pos: string; ko: string; phonetic?: string };
export type Example = { en: string; ko: string };
export type Word = { word: string; phonetic: string; senses: Sense[]; examples: Example[] };

export const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

export function el(tag: string, className: string, text: string) {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

/** #word · #phonetic · #senses · #examples 에 단어를 그린다 */
export function renderWord(w: Word) {
  $("word").textContent = w.word;
  $("phonetic").textContent = w.phonetic;
  $("senses").replaceChildren(
    ...w.senses.map((s) => {
      const li = el("li", "sense", "");
      li.append(el("span", "pos", s.pos));
      // 이 뜻만 발음이 다르면 뜻 앞에 따로 보여 준다 (예: close 형용사 /kloʊs/)
      if (s.phonetic) li.append(el("span", "phon", s.phonetic));
      li.append(el("span", "ko", s.ko));
      return li;
    }),
  );
  $("examples").replaceChildren(
    ...w.examples.map((e) => {
      const box = el("div", "example", "");
      box.append(el("p", "en", e.en), el("p", "ko", e.ko));
      return box;
    }),
  );
}
