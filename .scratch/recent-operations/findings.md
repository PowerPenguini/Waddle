# Operacje w Recent i wynikach wyszukiwania

## Przyczyna

Dostępność operacji wyznaczały niezależne warunki w obsłudze klawiatury,
menu i wykonaniu operacji. `folder_displayed()` służyło jednocześnie jako
warunek operacji na istniejących plikach i dostępności folderu docelowego.
Recent zawiera pełne ścieżki plików, ale nie jest folderem docelowym.
Wyniki wyszukiwania rekurencyjnego mają tę samą właściwość.

## Znalezione i poprawione przypadki

- Recent blokowało `d`, `y`, `x` i Delete. Komunikat błędnie wskazywał fokus
  bocznego panelu. Obsługa operacji na plikach obejmuje teraz Recent i wyniki
  wyszukiwania; `dd` nadal oznacza wycięcie, Delete przeniesienie do kosza.
- Menu nie oferowało zmiany nazwy ani przeniesienia do kosza w tych widokach.
  Menu i wykonanie korzystają teraz z tej samej reguły dostępności.
- Ctrl+C omijało ograniczenia stosowane przez `y`. Oba skróty respektują
  teraz fokus i dostępność operacji.
- Anulowanie wycięcia oraz kopiowanie zastępujące wycięcie odświeżały zwykły
  folder. Teraz odświeżają wyświetlaną lokalizację.
- Odświeżenie Recent i wyszukiwania pokazywało ponownie pliki oczekujące na
  wklejenie po wycięciu. Wszystkie trzy widoki filtrują te ścieżki przed
  odtworzeniem zaznaczenia.
- Undo/Redo wymagały zwykłego folderu, mimo że zapis operacji zawiera pełne
  ścieżki. Historia jest teraz dostępna niezależnie od rodzaju widoku,
  z zachowaniem blokad trwających operacji i konfliktów.
- Upuszczenie plików na puste tło kolekcji wskazywało niewidoczny poprzedni
  folder. Tło Recent, kosza i wyników wyszukiwania nie jest już celem
  przenoszenia. Foldery w koszu nie są zwykłymi celami zapisu.

Reguły są skupione w `src/app/operation_access.rs`. Rozróżniają operacje
na wskazanych plikach, zapis do bieżącego folderu, operacje kosza i historię.
Tworzenie i wklejanie pozostają niedostępne w kolekcjach bez folderu docelowego.

## Weryfikacja

Testy odtwarzające awarie przed poprawkami:

- `cargo test recent_supports_file_operators_and_delete_with_entries_focus -- --nocapture`
  kończył się błędem `d was rejected in Recent`.
- `cargo test collection_cut_refresh_and_cancel -- --nocapture`
  wykazał powrót wyciętego pliku, osobno w Recent i wyszukiwaniu.
- `cargo test collection_background_is_not_a_drop_destination -- --nocapture`
  zwracał ukryty folder zamiast braku celu.

Po rozszerzonym audycie `cargo test --all-targets --quiet`: 731 zaliczone,
0 błędów, 24 pominięte. Clippy z `-D warnings`, kontrola formatowania
i `git diff --check` przeszły. Testy na plikach tymczasowych obejmują również
zmianę nazwy, Undo, wycięcie i rzeczywiste przeniesienie do innego folderu.
Usuwanie z Recent sprawdzono również z rzeczywistym koszem GIO w osobnym
procesie z izolowanymi katalogami XDG. Pliki testowe znajdują się na tym
samym systemie plików co katalog domowy, ponieważ `/tmp` na tej maszynie
jest osobnym tmpfs i nie zapewnia tego samego kosza.

## Dodatkowy audyt mechanizmów

Potwierdzone regresjami i naprawione:

- Wynik odczytu Recent sprzed zakończenia przenoszenia mógł przywrócić
  nieaktualne wpisy. Odkładanie odświeżenia pamięta teraz rodzaj lokalizacji,
  a wykonanie odłożonego odświeżenia respektuje Recent i kosz.
- Odświeżanie kolekcji odrzucało zdarzenia podczas aktywnej operacji
  modyfikującej pliki. Odczyt listy może teraz odbywać się równolegle,
  tak jak w zwykłym folderze; kolejne zmiany nie giną.
- Po zniknięciu ostatniego pliku z danego folderu Recent przestawało
  obserwować ten folder. Przywrócenie pliku poza Waddle pozostawało
  niewidoczne. Obserwowane są również foldery brakujących wpisów historii.
- Zastąpienie wycięcia A wycięciem B nie przywracało A do widoku.
  Transfer session zgłasza teraz konieczność odtworzenia poprzednich wpisów;
  działa to w folderze, Recent i wyszukiwaniu.
- Czyszczenie lub wyłączenie Recent nie unieważniało odczytu rozpoczętego
  wcześniej. Polecenia anulują teraz wyłącznie odczyty Recent.
- Wyłączenie Recent podczas przechodzenia do innego folderu zastępowało
  wybrany cel poprzednim folderem. Nowsza nawigacja do folderu jest zachowana.

Nowe testy integracyjne w `src/app/tests/recent_lifecycle.rs` sprawdzają:

| Mechanizm | Sprawdzony wynik |
| --- | --- |
| Recent → kosz → Undo → Redo | Zgodność rzeczywistych plików, metadanych kosza i widoku |
| Przywrócenie z kosza → Undo → Redo | Przywracanie właściwej ścieżki i odświeżenie kosza |
| Podmiana pliku przed Undo | Odmowa naruszenia nowego pliku pod tą samą ścieżką |
| Kopiowanie i przenoszenie z wielu folderów | Zachowanie zawartości przy identycznych nazwach |
| Konflikt → zachowaj oba → Undo → Redo | Poprawne ścieżki i zawartość obu plików |
| Częściowy transfer → anulowanie → ponowienie | Osobne Undo części wykonanej i ponowionej, zachowany plik docelowy |
| Transfer/Undo/przywrócenie podczas odczytu listy | Starszy wynik nie pozostawia nieaktualnej listy |
| Zewnętrzne usunięcie i przywrócenie | Recent ponownie pokazuje przywrócony plik |
| Kolejne wycięcia | Poprzedni plik wraca do widoku, nowy pozostaje wycięty |
| Czyszczenie/wyłączenie podczas nawigacji | Stare wyniki są ignorowane, nowy cel nawigacji zachowany |

Nie przeprowadzono ręcznego testu interfejsu graficznego ani instalacji
nowej wersji aplikacji.
