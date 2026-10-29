// 퀴즈 창 — 외운 단어 전체에서 하나씩 뽑는다(맞힌 단어일수록 덜 나옴). 답은 눌러야 보인다.
// 방향: 영어 → 뜻 / 뜻 → 영어. [틀렸어요]는 그 단어를 "못 외웠어요"로 되돌린다.
import { invoke } from "@tauri-apps/api/core";
import { $, renderWord, speak, type Word } from "./word";

type MemorizedWord = { key: string; word: Word };

const quiz = $("quiz");
const question = $("question");
const reveal = $("reveal");
const grade = $("grade");
const book = $<HTMLSelectElement>("book");
const dir = $<HTMLSelectElement>("dir");
let current: MemorizedWord | null = null;
let right = 0;
let wrong = 0;

function setRevealed(revealed: boolean) {
  quiz.classList.toggle("revealed", revealed);
  reveal.hidden = revealed;
  grade.hidden = !revealed;
}

async function show() {
  current = await invoke<MemorizedWord | null>("quiz_word", { book: book.value || null });
  question.hidden = !current;
  setRevealed(false);
  if (!current) grade.hidden = true;
  $("empty").hidden = current !== null;
  $("score").hidden = !current;
  $("score").textContent = `맞음 ${right} · 틀림 ${wrong}`;
  if (current) renderWord(current.word);
}

async function answerWith(correct: boolean) {
  if (!current) return;
  if (correct) right++;
  else wrong++;
  await invoke("quiz_answer", { key: current.key, correct });
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
book.addEventListener("change", show);
// 방향을 바꾸면 지금 문제를 그대로 두고 가리는 쪽만 바꾼다
dir.addEventListener("change", () => {
  setDir();
  setRevealed(false);
});

async function start() {
  const books = await invoke<{ id: string; title: string }[]>("book_list");
  book.append(...books.map((b) => new Option(b.title, b.id)));
  setDir();
  show();
}
start();
