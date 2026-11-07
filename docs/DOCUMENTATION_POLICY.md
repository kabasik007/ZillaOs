# ZillaOS — правила документації та керування знаннями

**Статус:** прийнята документаційна конвенція I-M2, 2026-11-07.  
**Мета:** новий агент за 2–3 документи знаходить правду про систему, а не читає десятки застарілих планів.

## Одне джерело на кожен тип інформації

| Що саме потрібно | Єдине місце правди |
| --- | --- |
| Пояснення проєкту та швидкий старт | [кореневий README](../README.md) |
| Документальний маршрутизатор | [docs/README.md](README.md) |
| Датований реєстр реалізації | [STATUS.md](STATUS.md) |
| Поточні шари / власники / contract boundaries | [ARCHITECTURE_CURRENT.md](ARCHITECTURE_CURRENT.md) |
| Довгострокова архітектура й альтернативи | [ARCHITECTURE.md](ARCHITECTURE.md), [DECISIONS.md](DECISIONS.md) |
| Вимоги до поведінки | [docs/specs/features/](specs/README.md) |
| Результати перевірок | [verification/EVIDENCE_MATRIX.md](verification/EVIDENCE_MATRIX.md), GitHub Actions |
| Наступні задачі й залежності | [Mission Control #25](https://github.com/kabasik007/ZillaOs/issues/25), [NEXT_WAVE_I_M2.md](NEXT_WAVE_I_M2.md) |
| Контракти для агентів | [AGENTS.md](../AGENTS.md), [AGENT_COORDINATION.md](AGENT_COORDINATION.md) |
| Історичні рішення / старі інциденти | `docs/DECISIONS.md`, `docs/INTEGRATION_AUDIT_*.md`, `docs/CHANGELOG.md` |

Не дублюй одні й ті самі "актуальні" таблиці по десятьох файлах. Головна сторінка має **посилатися**, а не відтворювати усю документацію.

## Класифікація документів

- **Current / canonical:** `STATUS.md`, `ARCHITECTURE_CURRENT.md`, `DEVELOPMENT.md`, `AGENTS.md`. Оновлюються разом із перевіреними функціональними змінами.
- **Normative contract:** `specs/features/*.md`, security invariants, API/ABI rules. Змінюються тільки явним spec/ADR review та тестами.
- **Historical snapshot:** початкові milestone-документи, `INTEGRATION_AUDIT_*.md`, branch-specific I-M1 матеріали, старі частини `ROADMAP.md`, `CHANGELOG.md`. Дата і SHA мають значення; не використовувати їх як live state.
- **Runbook:** збірка, інсталяція, емулятори, діагностика. Команди перевіряються, а відсутні фізичні тести позначаються `NOT RUN`.
- **Proposed / research:** довгострокова архітектура, AMD hardware research. Не видається за готовий код.

## Fact → spec → design → evidence

1. **Discover:** перевірити source paths, контракти, поточний SHA і CI. Якщо документація суперечить коду, спочатку перевірити код і тести; не переписувати реалізацію під старий текст.
2. **Scope:** для нетривіальної зміни створити/оновити `specs/features/<slug>.md`, де окремо goal, non-goals, observable behavior, invariants, failure/edge cases, verification.
3. **Decide:** якщо потрібно змінити власника стану, ABI, безпекову межу або persisted data, зафіксувати ADR у `DECISIONS.md` чи окремій майбутній `decisions/` папці.
4. **Implement:** окрема агентська гілка від живого `main`, конкретні файли, негативні тести, ніяких чужих перезаписів.
5. **Verify:** зафіксувати команду, target, CI URL, commit SHA, PASS/FAIL/NOT RUN. Для runtime-claims потрібні QEMU або physical-lab логи, не тільки host unit.
6. **Handoff:** оновити відповідний доменний документ, `STATUS.md` за наявності нової перевіреної можливості, а README — тільки якщо змінюється користувацька картина.

## Перехід від старої пласкої документації

Ми **не переносимо й не видаляємо** десятки чинних `docs/*.md` одним комітом. Інакше зламаємо історичні посилання, агентські контексти і GitHub issues.

Цільова організація **для нових матеріалів**:

```text
docs/
  README.md                    навігація
  STATUS.md                    актуальний рівень можливостей
  ARCHITECTURE_CURRENT.md      живий архітектурний зріз
  DEVELOPMENT.md               як працювати з repo
  DOCUMENTATION_POLICY.md      це правило
  BOOTSTRAP_ADOPTION.md        як використано spec-first
  specs/
    README.md
    templates/feature-spec.md
    features/                   behaviour contracts
  verification/
    EVIDENCE_MATRIX.md          тестові гейти / provenance
  ... existing *.md            залишаються доступними
```

`docs/architecture/`, `docs/decisions/`, `docs/runbooks/`, `docs/historical/` можна створити пізніше **окремими міграціями з перевіркою inbound links**. До того підтримуємо чітку навігацію.

## Мінімальний metadata header для нових документів

Вгорі вкажи **status**, **as-of date**, **owner**, **source/evidence** (для фактів), **supersedes** (якщо замінює старий документ). Не прив'язуй кожну норму до назви старої гілки: source-of-truth = latest `main` і посилання на конкретну перевірку.

**Спростування:** при знайденій невідповідності не відмічай "verified" за інерцією — відкрий issue та явно позначи недостатній доказ.
