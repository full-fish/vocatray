// 퀴즈 창 — 외운 단어 전체에서 하나씩 뽑는다(맞힌 단어일수록 덜 나옴). 뜻은 눌러야 보인다.
// [틀렸어요]는 그 단어를 "못 외웠어요"로 되돌린다.
import { invoke } from "@tauri-apps/api/core";
import { $, renderWord, type Word } from "./word";

type MemorizedWord = { key: string; word: Word };

const question = $("question");
const answer = $("answer");
const reveal = $("reveal");
const grade = $("grade");
const book = $<HTMLSelectElement>("book");
let current: MemorizedWord | null = null;
let right = 0;
let wrong = 0;

async function show() {
  current = await invoke<MemorizedWord | null>("quiz_word", { book: book.value || null });
  question.hidden = !current;
  answer.hidden = true;
  reveal.hidden = false;
  grade.hidden = true;
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

reveal.addEventListener("click", () => {
  reveal.hidden = true;
  answer.hidden = false;
  grade.hidden = false;
});
$("right").addEventListener("click", () => answerWith(true));
$("wrong").addEventListener("click", () => answerWith(false));
book.addEventListener("change", show);

async function start() {
  const books = await invoke<{ id: string; title: string }[]>("book_list");
  book.append(...books.map((b) => new Option(b.title, b.id)));
  show();
}
start();
