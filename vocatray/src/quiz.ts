// 퀴즈 창 — 외운 단어 전체에서 하나씩 뽑는다(맞힌 단어일수록 덜 나옴). 답은 눌러야 보인다.
// 방향: 영어 → 뜻 / 뜻 → 영어 / 뜻 → 영어 쓰기. [틀렸어요]와 쓰기 오답은 그 단어를 "못 외웠어요"로 되돌린다.
import { invoke } from "@tauri-apps/api/core";
import "./popup";
import { $, el, renderWord, speak, type Word } from "./word";

type MemorizedWord = { key: string; word: Word };

const quiz = $("quiz");
const question = $("question");
const reveal = $("reveal");
const grade = $("grade");
const book = $<HTMLSelectElement>("book");
const dir = $<HTMLSelectElement>("dir");
const spell = $<HTMLFormElement>("spell");
const answer = $<HTMLInputElement>("answer");
const result = $("result");
const after = $("after");
const next = $("next");
let current: MemorizedWord | null = null;
let right = 0;
let wrong = 0;

const isSpell = () => dir.value === "spell";

function setRevealed(revealed: boolean) {
  quiz.classList.toggle("revealed", revealed);
  reveal.hidden = revealed || isSpell();
  grade.hidden = !revealed || isSpell();
  spell.hidden = revealed || !isSpell();
  result.hidden = !revealed || !isSpell();
  after.hidden = !revealed || !isSpell();
}

// 예문에 불규칙형으로 들어간 단어 — 규칙 변화로는 못 찾는다. 예문을 불규칙형으로 바꾸면 여기에도 더한다
const IRREGULAR = new Map(
  Object.entries({
    get: ["got", "gotten"],
    tooth: ["teeth"],
    blow: ["blew", "blown"],
    dig: ["dug"],
    hide: ["hid", "hidden"],
    leaf: ["leaves"],
    shoot: ["shot"],
    sink: ["sank", "sunk"],
    slide: ["slid"],
    spend: ["spent"],
    spin: ["spun"],
    steal: ["stole", "stolen"],
    cling: ["clung"],
    creep: ["crept"],
    overcome: ["overcame"],
    undertake: ["undertook", "undertaken"],
  }),
);

/** 예문에서 단어 자리를 빈칸으로 바꾼다. 그대로 없으면 변화형(adults, decided, applied)을 찾는다.
 *  빈칸에 들어갈 말이 blew여도 정답은 기본형 blow다. 그래도 못 찾으면 null */
function blankWord(sentence: string, word: string): string | null {
  const esc = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const forms = [word, ...(IRREGULAR.get(word) ?? [])].map(esc).join("|");
  const stems = [word, word.replace(/e$/, ""), word.replace(/y$/, "i")].map(esc).join("|");
  // 그대로 있는 걸 먼저 — a 문제에서 and, an까지 가리지 않게
  for (const re of [new RegExp(`\\b(?:${forms})\\b`, "gi"), new RegExp(`\\b(?:${stems})[a-z]{1,4}\\b`, "gi")]) {
    const blanked = sentence.replace(re, "____");
    if (blanked !== sentence) return blanked;
  }
  return null;
}

/** 뜻 → 영어 문제에 보여 줄 빈칸 예문을 예문마다 붙여 둔다. 언제 보일지는 pages.css가 정한다 */
function addBlanks(w: Word) {
  $("examples")
    .querySelectorAll(".example")
    .forEach((box, i) => {
      const blanked = blankWord(w.examples[i].en, w.word);
      if (blanked) box.prepend(el("p", "blank", blanked));
    });
}

function showScore() {
  $("score").textContent = `맞음 ${right} · 틀림 ${wrong}`;
}

async function show() {
  current = await invoke<MemorizedWord | null>("quiz_word", { book: book.value || null });
  question.hidden = !current;
  question.scrollTop = 0;
  setRevealed(false);
  if (!current) grade.hidden = true;
  $("empty").hidden = current !== null;
  $("score").hidden = !current;
  showScore();
  answer.value = "";
  if (current) {
    renderWord(current.word);
    addBlanks(current.word);
  }
  if (current && isSpell()) answer.focus();
}

async function record(correct: boolean) {
  if (!current) return;
  if (correct) right++;
  else wrong++;
  showScore();
  await invoke("quiz_answer", { key: current.key, correct });
}

async function answerWith(correct: boolean) {
  await record(correct);
  show();
}

function setDir() {
  quiz.dataset.dir = dir.value;
  reveal.textContent = dir.value === "en" ? "눌러서 뜻 보기" : "눌러서 영어 보기";
}

reveal.addEventListener("click", () => setRevealed(true));
$("speak").addEventListener("click", () => current && speak(current.word.word));
$("right").addEventListener("click", () => answerWith(true));
$("wrong").addEventListener("click", () => answerWith(false));
// 쓰기 — 대소문자만 다른 건 맞은 걸로 친다 (단어장의 I처럼). 채점은 바로 하고 결과를 보여 준 채 [다음]을 기다린다.
spell.addEventListener("submit", (e) => {
  e.preventDefault();
  const typed = answer.value.trim();
  if (!current || !typed) return;
  const correct = typed.toLowerCase() === current.word.word.toLowerCase();
  result.textContent = correct ? "⭕ 맞았어요" : `✗ 틀렸어요 · 내 답: ${typed}`;
  result.classList.toggle("wrong", !correct);
  setRevealed(true);
  next.focus();
  record(correct);
});
next.addEventListener("click", show);
book.addEventListener("change", () => {
  localStorage.setItem("quiz-book", book.value);
  show();
});
// 방향을 바꾸면 지금 문제를 그대로 두고 가리는 쪽만 바꾼다.
// 쓰기로 들어가거나 나올 때만 새 문제 — 이미 본 단어를 쓰거나 채점한 문제를 또 채점하지 않게.
dir.addEventListener("change", () => {
  const newQuestion = isSpell() || quiz.dataset.dir === "spell";
  localStorage.setItem("quiz-dir", dir.value);
  setDir();
  if (newQuestion) show();
  else setRevealed(false);
});

async function start() {
  const books = await invoke<{ id: string; title: string }[]>("book_list");
  book.append(...books.map((b) => new Option(b.title, b.id)));
  // 작은 창은 포커스를 잃을 때마다 닫히므로 고른 범위·방향을 기억해 둔다
  book.value = localStorage.getItem("quiz-book") ?? "";
  dir.value = localStorage.getItem("quiz-dir") ?? "en";
  setDir();
  show();
}
start();
