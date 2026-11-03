-- ERM test schema for MySQL 9.7 and MariaDB 11.8 (step 0.8 of docs/plan-erm-phase-0.md).
-- Same content as postgres.sql. Every table says which import case it covers.
-- Loaded by tools/dumps.sh into the database `flug`; the dumps are the test data, not this file.

SET NAMES utf8mb4;

-- Case: flight example (screenshot). Plain entity, PK `id`.
CREATE TABLE kunde (
    id        INT          NOT NULL AUTO_INCREMENT,
    vorname   VARCHAR(50)  NOT NULL,
    nachname  VARCHAR(50)  NOT NULL,
    adresse   VARCHAR(200),
    email     VARCHAR(120) NOT NULL COMMENT 'login address, unique',
    telefon   VARCHAR(30),
    PRIMARY KEY (id),
    UNIQUE KEY uq_kunde_email (email)
) ENGINE=InnoDB COMMENT='Customer who books tickets';

-- Case: flight example. Referenced by flugzeug (1:n NOT NULL).
CREATE TABLE fluggesellschaft (
    id       INT          NOT NULL AUTO_INCREMENT,
    name     VARCHAR(100) NOT NULL,
    adresse  VARCHAR(200),
    telefon  VARCHAR(30),
    email    VARCHAR(120),
    PRIMARY KEY (id)
) ENGINE=InnoDB;

-- Case: flight example; 1:n NOT NULL with the FK declared inline as a table constraint.
CREATE TABLE flugzeug (
    id                   INT          NOT NULL AUTO_INCREMENT,
    modell               VARCHAR(60)  NOT NULL,
    sitze                SMALLINT UNSIGNED NOT NULL,
    geschwindigkeit      DECIMAL(6,1) COMMENT 'cruise speed in km/h',
    fluggesellschaft_id  INT          NOT NULL,
    PRIMARY KEY (id),
    CONSTRAINT fk_flugzeug_gesellschaft FOREIGN KEY (fluggesellschaft_id)
        REFERENCES fluggesellschaft (id)
) ENGINE=InnoDB;

-- Case: flight example; target of two FKs from flug (start/ziel).
CREATE TABLE flughafen (
    id     CHAR(3)      NOT NULL COMMENT 'IATA code',
    name   VARCHAR(100) NOT NULL,
    stadt  VARCHAR(80)  NOT NULL,
    land   VARCHAR(80)  NOT NULL,
    PRIMARY KEY (id)
) ENGINE=InnoDB;

-- Case: composite PK (flughafen_id, nummer); weak by PK = FK + own column as well.
CREATE TABLE gate (
    flughafen_id  CHAR(3)     NOT NULL,
    nummer        VARCHAR(5)  NOT NULL,
    terminal      VARCHAR(5),
    PRIMARY KEY (flughafen_id, nummer),
    CONSTRAINT fk_gate_flughafen FOREIGN KEY (flughafen_id) REFERENCES flughafen (id)
) ENGINE=InnoDB;

-- Case: flight example; two FKs to the same table (start/ziel), one nullable composite FK
-- (gate_*), a generated column. FKs are added later with ALTER TABLE.
CREATE TABLE flug (
    id                   INT       NOT NULL AUTO_INCREMENT,
    nummer               VARCHAR(8) NOT NULL,
    preis                DECIMAL(8,2) NOT NULL DEFAULT 0.00,
    start_zeit           DATETIME  NOT NULL,
    end_zeit             DATETIME  NOT NULL,
    dauer_min            INT GENERATED ALWAYS AS (TIMESTAMPDIFF(MINUTE, start_zeit, end_zeit))
                         VIRTUAL,
    flugzeug_id          INT       NOT NULL,
    start_flughafen_id   CHAR(3)   NOT NULL,
    ziel_flughafen_id    CHAR(3)   NOT NULL,
    gate_flughafen_id    CHAR(3),
    gate_nummer          VARCHAR(5),
    PRIMARY KEY (id),
    KEY ix_flug_start (start_flughafen_id, start_zeit)
) ENGINE=InnoDB;

ALTER TABLE flug
    ADD CONSTRAINT fk_flug_flugzeug FOREIGN KEY (flugzeug_id) REFERENCES flugzeug (id),
    ADD CONSTRAINT fk_flug_start FOREIGN KEY (start_flughafen_id) REFERENCES flughafen (id),
    ADD CONSTRAINT fk_flug_ziel FOREIGN KEY (ziel_flughafen_id) REFERENCES flughafen (id);
ALTER TABLE flug ADD CONSTRAINT fk_flug_gate FOREIGN KEY (gate_flughafen_id, gate_nummer)
    REFERENCES gate (flughafen_id, nummer) ON DELETE SET NULL;

-- Case: flight example; two 1:n NOT NULL FKs (kunde bucht, flug reserviert), ENUM column.
CREATE TABLE ticket (
    id         INT NOT NULL AUTO_INCREMENT,
    status     ENUM('reserviert','bezahlt','storniert') NOT NULL DEFAULT 'reserviert',
    sitzplatz  VARCHAR(4),
    kunde_id   INT NOT NULL,
    flug_id    INT NOT NULL,
    PRIMARY KEY (id),
    CONSTRAINT fk_ticket_kunde FOREIGN KEY (kunde_id) REFERENCES kunde (id) ON DELETE CASCADE,
    CONSTRAINT fk_ticket_flug FOREIGN KEY (flug_id) REFERENCES flug (id)
) ENGINE=InnoDB;

-- Case: 1:1 by a UNIQUE NOT NULL FK.
CREATE TABLE kunde_konto (
    id             INT NOT NULL AUTO_INCREMENT,
    kunde_id       INT NOT NULL,
    login          VARCHAR(40) NOT NULL,
    passwort_hash  VARBINARY(64) NOT NULL,
    PRIMARY KEY (id),
    UNIQUE KEY uq_konto_kunde (kunde_id),
    CONSTRAINT fk_konto_kunde FOREIGN KEY (kunde_id) REFERENCES kunde (id)
) ENGINE=InnoDB;

-- Case: PK = FK (1:1 extension, O5): plain binary relationship, no IS-A.
CREATE TABLE vielflieger (
    kunde_id  INT NOT NULL,
    meilen    INT NOT NULL DEFAULT 0,
    stufe     VARCHAR(10) NOT NULL DEFAULT 'basis',
    PRIMARY KEY (kunde_id),
    CONSTRAINT fk_vielflieger_kunde FOREIGN KEY (kunde_id) REFERENCES kunde (id)
) ENGINE=InnoDB;

-- Case: self-reference (nullable 1:n to the same table).
CREATE TABLE mitarbeiter (
    id               INT NOT NULL AUTO_INCREMENT,
    name             VARCHAR(100) NOT NULL,
    vorgesetzter_id  INT,
    PRIMARY KEY (id),
    CONSTRAINT fk_mitarbeiter_chef FOREIGN KEY (vorgesetzter_id) REFERENCES mitarbeiter (id)
) ENGINE=InnoDB;

-- Case: n:m without extra columns (junction table, PK = both FKs).
CREATE TABLE flug_crew (
    flug_id         INT NOT NULL,
    mitarbeiter_id  INT NOT NULL,
    PRIMARY KEY (flug_id, mitarbeiter_id),
    CONSTRAINT fk_crew_flug FOREIGN KEY (flug_id) REFERENCES flug (id),
    CONSTRAINT fk_crew_mitarbeiter FOREIGN KEY (mitarbeiter_id) REFERENCES mitarbeiter (id)
) ENGINE=InnoDB;

-- Referenced by buchung_extra.
CREATE TABLE extra (
    id           INT NOT NULL AUTO_INCREMENT PRIMARY KEY,
    bezeichnung  VARCHAR(60) NOT NULL,
    preis        DECIMAL(8,2) NOT NULL
) ENGINE=InnoDB;

-- Case: n:m with extra columns (anzahl, preis become relationship attributes); ticket_id uses an
-- inline column REFERENCES, which MySQL 9.7 and MariaDB 11.8 both turn into a FK.
CREATE TABLE buchung_extra (
    ticket_id  INT NOT NULL REFERENCES ticket (id),
    extra_id   INT NOT NULL,
    anzahl     SMALLINT NOT NULL DEFAULT 1,
    preis      DECIMAL(8,2) NOT NULL,
    PRIMARY KEY (ticket_id, extra_id),
    FOREIGN KEY (extra_id) REFERENCES extra (id)
) ENGINE=InnoDB;

-- Case: ternary relationship (junction table with three FKs).
CREATE TABLE pilot (
    id      INT NOT NULL AUTO_INCREMENT,
    name    VARCHAR(100) NOT NULL,
    lizenz  VARCHAR(20) NOT NULL,
    PRIMARY KEY (id)
) ENGINE=InnoDB;

CREATE TABLE rolle (
    id           INT NOT NULL AUTO_INCREMENT,
    bezeichnung  VARCHAR(40) NOT NULL,
    PRIMARY KEY (id)
) ENGINE=InnoDB;

CREATE TABLE einsatz (
    pilot_id  INT NOT NULL,
    flug_id   INT NOT NULL,
    rolle_id  INT NOT NULL,
    stunden   DECIMAL(4,1),
    PRIMARY KEY (pilot_id, flug_id, rolle_id),
    CONSTRAINT fk_einsatz_pilot FOREIGN KEY (pilot_id) REFERENCES pilot (id),
    CONSTRAINT fk_einsatz_flug FOREIGN KEY (flug_id) REFERENCES flug (id),
    CONSTRAINT fk_einsatz_rolle FOREIGN KEY (rolle_id) REFERENCES rolle (id)
) ENGINE=InnoDB;

-- Case: weak entity (PK = FK + own columns reihe, buchstabe = partial key).
CREATE TABLE sitzplatz (
    flugzeug_id  INT NOT NULL,
    reihe        TINYINT UNSIGNED NOT NULL,
    buchstabe    CHAR(1) NOT NULL,
    klasse       ENUM('economy','business','first') NOT NULL DEFAULT 'economy',
    PRIMARY KEY (flugzeug_id, reihe, buchstabe),
    CONSTRAINT fk_sitzplatz_flugzeug FOREIGN KEY (flugzeug_id) REFERENCES flugzeug (id)
        ON DELETE CASCADE
) ENGINE=InnoDB;

-- Case: quoting with spaces and capitals, umlaut in a table name, table without PK,
-- FK whose target is a UNIQUE column instead of the PK.
CREATE TABLE `Fracht Stück` (
    `Sendungs Nr`  INT NOT NULL,
    `Gewicht kg`   DECIMAL(7,2) NOT NULL,
    flug_nummer    VARCHAR(8),
    kunde_email    VARCHAR(120),
    CONSTRAINT `fk_fracht_kunde` FOREIGN KEY (kunde_email) REFERENCES kunde (email)
) ENGINE=InnoDB COMMENT='Cargo item; no primary key on purpose';

-- Case: view, trigger, procedure (must be skipped by the importer).
CREATE VIEW flugplan AS
    SELECT f.nummer, f.start_flughafen_id AS von, f.ziel_flughafen_id AS nach, f.start_zeit
    FROM flug f;

DELIMITER //
CREATE TRIGGER ticket_storno BEFORE UPDATE ON ticket
FOR EACH ROW
BEGIN
    IF NEW.status = 'storniert' THEN
        SET NEW.sitzplatz = NULL;
    END IF;
END//

CREATE PROCEDURE freie_plaetze(IN p_flug INT, OUT p_frei INT)
BEGIN
    -- comment with ; inside a body
    SELECT z.sitze - COUNT(t.id) INTO p_frei
    FROM flug f JOIN flugzeug z ON z.id = f.flugzeug_id
    LEFT JOIN ticket t ON t.flug_id = f.id AND t.status <> 'storniert'
    WHERE f.id = p_flug
    GROUP BY z.sitze;
END//
DELIMITER ;

-- Sample rows (so the dumps with data contain INSERTs with quotes, NULL, umlauts, binary).
INSERT INTO kunde (vorname, nachname, adresse, email, telefon) VALUES
    ('Anna', 'Müller', 'Hauptstraße 1; 10115 Berlin', 'anna@example.org', NULL),
    ('Bob', 'O''Brien', 'Line 1\nLine 2', 'bob@example.org', '+49 30 123');
INSERT INTO fluggesellschaft (name) VALUES ('Lufthansa'), ('Condor');
INSERT INTO flugzeug (modell, sitze, geschwindigkeit, fluggesellschaft_id) VALUES
    ('A320', 180, 840.0, 1), ('B737', 189, 842.5, 2);
INSERT INTO flughafen VALUES ('BER', 'Berlin Brandenburg', 'Berlin', 'Deutschland'),
    ('MUC', 'Franz Josef Strauß', 'München', 'Deutschland');
INSERT INTO gate VALUES ('BER', 'A12', '1'), ('MUC', 'H3', '2');
INSERT INTO flug (nummer, preis, start_zeit, end_zeit, flugzeug_id, start_flughafen_id,
                  ziel_flughafen_id, gate_flughafen_id, gate_nummer) VALUES
    ('LH100', 129.99, '2026-11-02 07:00:00', '2026-11-02 08:05:00', 1, 'BER', 'MUC', 'BER', 'A12'),
    ('DE200', 89.00, '2026-11-03 18:30:00', '2026-11-03 19:40:00', 2, 'MUC', 'BER', NULL, NULL);
INSERT INTO ticket (status, sitzplatz, kunde_id, flug_id) VALUES
    ('bezahlt', '12A', 1, 1), ('reserviert', NULL, 2, 2);
INSERT INTO kunde_konto (kunde_id, login, passwort_hash) VALUES (1, 'anna', X'00FF10');
INSERT INTO vielflieger (kunde_id, meilen) VALUES (1, 12000);
INSERT INTO mitarbeiter (name, vorgesetzter_id) VALUES ('Chefin', NULL), ('Kai', 1);
INSERT INTO flug_crew VALUES (1, 1), (1, 2);
INSERT INTO extra (bezeichnung, preis) VALUES ('Koffer 23 kg', 35.00);
INSERT INTO buchung_extra VALUES (1, 1, 1, 35.00);
INSERT INTO pilot (name, lizenz) VALUES ('Pia', 'ATPL-1');
INSERT INTO rolle (bezeichnung) VALUES ('Kapitän'), ('Erster Offizier');
INSERT INTO einsatz VALUES (1, 1, 1, 1.5);
INSERT INTO sitzplatz (flugzeug_id, reihe, buchstabe) VALUES (1, 12, 'A'), (1, 12, 'B');
INSERT INTO `Fracht Stück` VALUES (1, 12.5, 'LH100', 'anna@example.org');
