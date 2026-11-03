| Art | Name | Attribute | Enden (look-across) |
|-----|------|-----------|---------------------|
| Entity | Fracht Stück | Sendungs Nr, Gewicht kg, flug_nummer | |
| schwache Entity | Buchung_extra | <u>ticket_id</u>˙, anzahl, preis | |
| Entity | Extra | <u>id</u>, bezeichnung, preis | |
| Entity | Flug | <u>id</u>, nummer, preis, start_zeit, end_zeit, dauer_min (abgeleitet) | |
| Entity | Fluggesellschaft | <u>id</u>, name, adresse, telefon, email | |
| Entity | Flughafen | <u>id</u>, name, stadt, land | |
| Entity | Flugzeug | <u>id</u>, modell, sitze, geschwindigkeit | |
| schwache Entity | Gate | <u>nummer</u>˙, terminal | |
| Entity | Kunde | <u>id</u>, vorname, nachname, adresse, email, telefon | |
| Entity | Kunde_konto | <u>id</u>, login, passwort_hash | |
| Entity | Mitarbeiter | <u>id</u>, name | |
| Entity | Pilot | <u>id</u>, name, lizenz | |
| Entity | Rolle | <u>id</u>, bezeichnung | |
| schwache Entity | Sitzplatz | <u>reihe</u>˙, <u>buchstabe</u>˙, klasse | |
| Entity | Ticket | <u>id</u>, status, sitzplatz | |
| Entity | Vielflieger | meilen, stufe | |
| Beziehung | kunde_email | – | Kunde [0,1]; Fracht Stück [0,n] |
| ident. Beziehung | hat | – | Extra [1,1]; Buchung_extra [0,n] ═ |
| Beziehung | einsatz | stunden | Flug [0,n]; Pilot [0,n]; Rolle [0,n] |
| Beziehung | hat | – | Flugzeug [1,1]; Flug [0,n] |
| Beziehung | hat | – | Gate [0,1]; Flug [0,n] |
| Beziehung | start_flughafen | – | Flughafen [1,1]; Flug [0,n] |
| Beziehung | ziel_flughafen | – | Flughafen [1,1]; Flug [0,n] |
| Beziehung | flug_crew | – | Flug [0,n]; Mitarbeiter [0,n] |
| Beziehung | hat | – | Fluggesellschaft [1,1]; Flugzeug [0,n] |
| ident. Beziehung | hat | – | Flughafen [1,1]; Gate [0,n] ═ |
| Beziehung | hat | – | Kunde [1,1]; Kunde_konto [0,1] |
| Beziehung | vorgesetzter | – | Mitarbeiter [0,1] (Rolle vorgesetzter); Mitarbeiter [0,n] |
| ident. Beziehung | hat | – | Flugzeug [1,1]; Sitzplatz [0,n] ═ |
| Beziehung | hat | – | Flug [1,1]; Ticket [0,n] |
| Beziehung | hat | – | Kunde [1,1]; Ticket [0,n] |
| Beziehung | hat | – | Kunde [1,1]; Vielflieger [0,1] |
