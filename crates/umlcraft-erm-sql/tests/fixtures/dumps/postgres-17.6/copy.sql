--
-- PostgreSQL database dump
--

\restrict KEY

-- Dumped from database version 17.6 (Debian 17.6-2.pgdg13+1)
-- Dumped by pg_dump version 17.6 (Debian 17.6-2.pgdg13+1)

SET statement_timeout = 0;
SET lock_timeout = 0;
SET idle_in_transaction_session_timeout = 0;
SET transaction_timeout = 0;
SET client_encoding = 'UTF8';
SET standard_conforming_strings = on;
SELECT pg_catalog.set_config('search_path', '', false);
SET check_function_bodies = false;
SET xmloption = content;
SET client_min_messages = warning;
SET row_security = off;

--
-- Name: fracht; Type: SCHEMA; Schema: -; Owner: postgres
--

CREATE SCHEMA fracht;


ALTER SCHEMA fracht OWNER TO postgres;

--
-- Name: SCHEMA fracht; Type: COMMENT; Schema: -; Owner: postgres
--

COMMENT ON SCHEMA fracht IS 'cargo';


--
-- Name: email_t; Type: DOMAIN; Schema: public; Owner: postgres
--

CREATE DOMAIN public.email_t AS character varying(120)
	CONSTRAINT email_t_check CHECK (((VALUE)::text ~~ '%@%'::text));


ALTER DOMAIN public.email_t OWNER TO postgres;

--
-- Name: ticket_status; Type: TYPE; Schema: public; Owner: postgres
--

CREATE TYPE public.ticket_status AS ENUM (
    'reserviert',
    'bezahlt',
    'storniert'
);


ALTER TYPE public.ticket_status OWNER TO postgres;

--
-- Name: freie_plaetze(integer); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.freie_plaetze(p_flug integer) RETURNS integer
    LANGUAGE sql
    AS $$
    SELECT z.sitze - count(t.id)::integer
    FROM flug f JOIN flugzeug z ON z.id = f.flugzeug_id
    LEFT JOIN ticket t ON t.flug_id = f.id AND t.status <> 'storniert'
    WHERE f.id = p_flug
    GROUP BY z.sitze;
$$;


ALTER FUNCTION public.freie_plaetze(p_flug integer) OWNER TO postgres;

--
-- Name: ticket_storno(); Type: FUNCTION; Schema: public; Owner: postgres
--

CREATE FUNCTION public.ticket_storno() RETURNS trigger
    LANGUAGE plpgsql
    AS $$
BEGIN
    IF NEW.status = 'storniert' THEN
        NEW.sitzplatz := NULL; -- comment with ; inside a body
    END IF;
    RETURN NEW;
END;
$$;


ALTER FUNCTION public.ticket_storno() OWNER TO postgres;

SET default_tablespace = '';

SET default_table_access_method = heap;

--
-- Name: Container; Type: TABLE; Schema: fracht; Owner: postgres
--

CREATE TABLE fracht."Container" (
    "ID" integer NOT NULL,
    flug_id integer NOT NULL,
    "Typ" text NOT NULL
);


ALTER TABLE fracht."Container" OWNER TO postgres;

--
-- Name: Container_ID_seq; Type: SEQUENCE; Schema: fracht; Owner: postgres
--

CREATE SEQUENCE fracht."Container_ID_seq"
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE fracht."Container_ID_seq" OWNER TO postgres;

--
-- Name: Container_ID_seq; Type: SEQUENCE OWNED BY; Schema: fracht; Owner: postgres
--

ALTER SEQUENCE fracht."Container_ID_seq" OWNED BY fracht."Container"."ID";


--
-- Name: Fracht Stück; Type: TABLE; Schema: fracht; Owner: postgres
--

CREATE TABLE fracht."Fracht Stück" (
    "Sendungs Nr" integer NOT NULL,
    "Gewicht kg" numeric(7,2) NOT NULL,
    flug_nummer character varying(8),
    kunde_email character varying(120)
);


ALTER TABLE fracht."Fracht Stück" OWNER TO postgres;

--
-- Name: TABLE "Fracht Stück"; Type: COMMENT; Schema: fracht; Owner: postgres
--

COMMENT ON TABLE fracht."Fracht Stück" IS 'Cargo item; no primary key on purpose';


--
-- Name: buchung_extra; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.buchung_extra (
    ticket_id integer NOT NULL,
    extra_id integer NOT NULL,
    anzahl smallint DEFAULT 1 NOT NULL,
    preis numeric(8,2) NOT NULL
);


ALTER TABLE public.buchung_extra OWNER TO postgres;

--
-- Name: einsatz; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.einsatz (
    pilot_id integer NOT NULL,
    flug_id integer NOT NULL,
    rolle_id integer NOT NULL,
    stunden numeric(4,1)
);


ALTER TABLE public.einsatz OWNER TO postgres;

--
-- Name: extra; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.extra (
    id integer NOT NULL,
    bezeichnung character varying(60) NOT NULL,
    preis numeric(8,2) NOT NULL
);


ALTER TABLE public.extra OWNER TO postgres;

--
-- Name: extra_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.extra_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.extra_id_seq OWNER TO postgres;

--
-- Name: extra_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.extra_id_seq OWNED BY public.extra.id;


--
-- Name: flug; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.flug (
    id integer NOT NULL,
    nummer character varying(8) NOT NULL,
    preis numeric(8,2) DEFAULT 0.00 NOT NULL,
    start_zeit timestamp without time zone NOT NULL,
    end_zeit timestamp without time zone NOT NULL,
    dauer_min integer GENERATED ALWAYS AS (((EXTRACT(epoch FROM (end_zeit - start_zeit)))::integer / 60)) STORED,
    flugzeug_id integer NOT NULL,
    start_flughafen_id character(3) NOT NULL,
    ziel_flughafen_id character(3) NOT NULL,
    gate_flughafen_id character(3),
    gate_nummer character varying(5)
);


ALTER TABLE public.flug OWNER TO postgres;

--
-- Name: flug_crew; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.flug_crew (
    flug_id integer NOT NULL,
    mitarbeiter_id integer NOT NULL
);


ALTER TABLE public.flug_crew OWNER TO postgres;

--
-- Name: flug_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.flug_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.flug_id_seq OWNER TO postgres;

--
-- Name: flug_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.flug_id_seq OWNED BY public.flug.id;


--
-- Name: fluggesellschaft; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.fluggesellschaft (
    id integer NOT NULL,
    name character varying(100) NOT NULL,
    adresse character varying(200),
    telefon character varying(30),
    email character varying(120)
);


ALTER TABLE public.fluggesellschaft OWNER TO postgres;

--
-- Name: fluggesellschaft_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.fluggesellschaft_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.fluggesellschaft_id_seq OWNER TO postgres;

--
-- Name: fluggesellschaft_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.fluggesellschaft_id_seq OWNED BY public.fluggesellschaft.id;


--
-- Name: flughafen; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.flughafen (
    id character(3) NOT NULL,
    name character varying(100) NOT NULL,
    stadt character varying(80) NOT NULL,
    land character varying(80) NOT NULL
);


ALTER TABLE public.flughafen OWNER TO postgres;

--
-- Name: COLUMN flughafen.id; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON COLUMN public.flughafen.id IS 'IATA code';


--
-- Name: flugplan; Type: VIEW; Schema: public; Owner: postgres
--

CREATE VIEW public.flugplan AS
 SELECT nummer,
    start_flughafen_id AS von,
    ziel_flughafen_id AS nach,
    start_zeit
   FROM public.flug f;


ALTER VIEW public.flugplan OWNER TO postgres;

--
-- Name: flugzeug; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.flugzeug (
    id integer NOT NULL,
    modell character varying(60) NOT NULL,
    sitze smallint NOT NULL,
    geschwindigkeit numeric(6,1),
    fluggesellschaft_id integer NOT NULL,
    CONSTRAINT flugzeug_sitze_check CHECK ((sitze > 0))
);


ALTER TABLE public.flugzeug OWNER TO postgres;

--
-- Name: COLUMN flugzeug.geschwindigkeit; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON COLUMN public.flugzeug.geschwindigkeit IS 'cruise speed in km/h';


--
-- Name: flugzeug_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.flugzeug_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.flugzeug_id_seq OWNER TO postgres;

--
-- Name: flugzeug_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.flugzeug_id_seq OWNED BY public.flugzeug.id;


--
-- Name: gate; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.gate (
    flughafen_id character(3) NOT NULL,
    nummer character varying(5) NOT NULL,
    terminal character varying(5)
);


ALTER TABLE public.gate OWNER TO postgres;

--
-- Name: kunde; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.kunde (
    id integer NOT NULL,
    vorname character varying(50) NOT NULL,
    nachname character varying(50) NOT NULL,
    adresse character varying(200),
    email public.email_t NOT NULL,
    telefon character varying(30)
);


ALTER TABLE public.kunde OWNER TO postgres;

--
-- Name: TABLE kunde; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON TABLE public.kunde IS 'Customer who books tickets';


--
-- Name: COLUMN kunde.email; Type: COMMENT; Schema: public; Owner: postgres
--

COMMENT ON COLUMN public.kunde.email IS 'login address, unique';


--
-- Name: kunde_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

ALTER TABLE public.kunde ALTER COLUMN id ADD GENERATED BY DEFAULT AS IDENTITY (
    SEQUENCE NAME public.kunde_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: kunde_konto; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.kunde_konto (
    id integer NOT NULL,
    kunde_id integer NOT NULL,
    login character varying(40) NOT NULL,
    passwort_hash bytea NOT NULL
);


ALTER TABLE public.kunde_konto OWNER TO postgres;

--
-- Name: kunde_konto_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.kunde_konto_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.kunde_konto_id_seq OWNER TO postgres;

--
-- Name: kunde_konto_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.kunde_konto_id_seq OWNED BY public.kunde_konto.id;


--
-- Name: mitarbeiter; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.mitarbeiter (
    id integer NOT NULL,
    name character varying(100) NOT NULL,
    vorgesetzter_id integer
);


ALTER TABLE public.mitarbeiter OWNER TO postgres;

--
-- Name: mitarbeiter_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.mitarbeiter_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.mitarbeiter_id_seq OWNER TO postgres;

--
-- Name: mitarbeiter_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.mitarbeiter_id_seq OWNED BY public.mitarbeiter.id;


--
-- Name: pilot; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.pilot (
    id integer NOT NULL,
    name character varying(100) NOT NULL,
    lizenz character varying(20) NOT NULL
);


ALTER TABLE public.pilot OWNER TO postgres;

--
-- Name: pilot_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.pilot_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.pilot_id_seq OWNER TO postgres;

--
-- Name: pilot_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.pilot_id_seq OWNED BY public.pilot.id;


--
-- Name: rolle; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.rolle (
    id integer NOT NULL,
    bezeichnung character varying(40) NOT NULL
);


ALTER TABLE public.rolle OWNER TO postgres;

--
-- Name: rolle_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

CREATE SEQUENCE public.rolle_id_seq
    AS integer
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1;


ALTER SEQUENCE public.rolle_id_seq OWNER TO postgres;

--
-- Name: rolle_id_seq; Type: SEQUENCE OWNED BY; Schema: public; Owner: postgres
--

ALTER SEQUENCE public.rolle_id_seq OWNED BY public.rolle.id;


--
-- Name: sitzplatz; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.sitzplatz (
    flugzeug_id integer NOT NULL,
    reihe smallint NOT NULL,
    buchstabe character(1) NOT NULL,
    klasse character varying(10) DEFAULT 'economy'::character varying NOT NULL,
    CONSTRAINT sitzplatz_klasse_check CHECK (((klasse)::text = ANY ((ARRAY['economy'::character varying, 'business'::character varying, 'first'::character varying])::text[])))
);


ALTER TABLE public.sitzplatz OWNER TO postgres;

--
-- Name: ticket; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.ticket (
    id integer NOT NULL,
    status public.ticket_status DEFAULT 'reserviert'::public.ticket_status NOT NULL,
    sitzplatz character varying(4),
    kunde_id integer NOT NULL,
    flug_id integer NOT NULL
);


ALTER TABLE public.ticket OWNER TO postgres;

--
-- Name: ticket_id_seq; Type: SEQUENCE; Schema: public; Owner: postgres
--

ALTER TABLE public.ticket ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (
    SEQUENCE NAME public.ticket_id_seq
    START WITH 1
    INCREMENT BY 1
    NO MINVALUE
    NO MAXVALUE
    CACHE 1
);


--
-- Name: vielflieger; Type: TABLE; Schema: public; Owner: postgres
--

CREATE TABLE public.vielflieger (
    kunde_id integer NOT NULL,
    meilen integer DEFAULT 0 NOT NULL,
    stufe character varying(10) DEFAULT 'basis'::character varying NOT NULL
);


ALTER TABLE public.vielflieger OWNER TO postgres;

--
-- Name: Container ID; Type: DEFAULT; Schema: fracht; Owner: postgres
--

ALTER TABLE ONLY fracht."Container" ALTER COLUMN "ID" SET DEFAULT nextval('fracht."Container_ID_seq"'::regclass);


--
-- Name: extra id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.extra ALTER COLUMN id SET DEFAULT nextval('public.extra_id_seq'::regclass);


--
-- Name: flug id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug ALTER COLUMN id SET DEFAULT nextval('public.flug_id_seq'::regclass);


--
-- Name: fluggesellschaft id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.fluggesellschaft ALTER COLUMN id SET DEFAULT nextval('public.fluggesellschaft_id_seq'::regclass);


--
-- Name: flugzeug id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flugzeug ALTER COLUMN id SET DEFAULT nextval('public.flugzeug_id_seq'::regclass);


--
-- Name: kunde_konto id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.kunde_konto ALTER COLUMN id SET DEFAULT nextval('public.kunde_konto_id_seq'::regclass);


--
-- Name: mitarbeiter id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.mitarbeiter ALTER COLUMN id SET DEFAULT nextval('public.mitarbeiter_id_seq'::regclass);


--
-- Name: pilot id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.pilot ALTER COLUMN id SET DEFAULT nextval('public.pilot_id_seq'::regclass);


--
-- Name: rolle id; Type: DEFAULT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.rolle ALTER COLUMN id SET DEFAULT nextval('public.rolle_id_seq'::regclass);


--
-- Data for Name: Container; Type: TABLE DATA; Schema: fracht; Owner: postgres
--

COPY fracht."Container" ("ID", flug_id, "Typ") FROM stdin;
1	1	AKE
\.


--
-- Data for Name: Fracht Stück; Type: TABLE DATA; Schema: fracht; Owner: postgres
--

COPY fracht."Fracht Stück" ("Sendungs Nr", "Gewicht kg", flug_nummer, kunde_email) FROM stdin;
1	12.50	LH100	anna@example.org
\.


--
-- Data for Name: buchung_extra; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.buchung_extra (ticket_id, extra_id, anzahl, preis) FROM stdin;
1	1	1	35.00
\.


--
-- Data for Name: einsatz; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.einsatz (pilot_id, flug_id, rolle_id, stunden) FROM stdin;
1	1	1	1.5
\.


--
-- Data for Name: extra; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.extra (id, bezeichnung, preis) FROM stdin;
1	Koffer 23 kg	35.00
\.


--
-- Data for Name: flug; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.flug (id, nummer, preis, start_zeit, end_zeit, flugzeug_id, start_flughafen_id, ziel_flughafen_id, gate_flughafen_id, gate_nummer) FROM stdin;
1	LH100	129.99	2026-11-02 07:00:00	2026-11-02 08:05:00	1	BER	MUC	BER	A12
2	DE200	89.00	2026-11-03 18:30:00	2026-11-03 19:40:00	2	MUC	BER	\N	\N
\.


--
-- Data for Name: flug_crew; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.flug_crew (flug_id, mitarbeiter_id) FROM stdin;
1	1
1	2
\.


--
-- Data for Name: fluggesellschaft; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.fluggesellschaft (id, name, adresse, telefon, email) FROM stdin;
1	Lufthansa	\N	\N	\N
2	Condor	\N	\N	\N
\.


--
-- Data for Name: flughafen; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.flughafen (id, name, stadt, land) FROM stdin;
BER	Berlin Brandenburg	Berlin	Deutschland
MUC	Franz Josef Strauß	München	Deutschland
\.


--
-- Data for Name: flugzeug; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.flugzeug (id, modell, sitze, geschwindigkeit, fluggesellschaft_id) FROM stdin;
1	A320	180	840.0	1
2	B737	189	842.5	2
\.


--
-- Data for Name: gate; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.gate (flughafen_id, nummer, terminal) FROM stdin;
BER	A12	1
MUC	H3	2
\.


--
-- Data for Name: kunde; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.kunde (id, vorname, nachname, adresse, email, telefon) FROM stdin;
1	Anna	Müller	Hauptstraße 1; 10115 Berlin	anna@example.org	\N
2	Bob	O'Brien	Line 1\nLine 2	bob@example.org	+49 30 123
\.


--
-- Data for Name: kunde_konto; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.kunde_konto (id, kunde_id, login, passwort_hash) FROM stdin;
1	1	anna	\\x00ff10
\.


--
-- Data for Name: mitarbeiter; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.mitarbeiter (id, name, vorgesetzter_id) FROM stdin;
1	Chefin	\N
2	Kai	1
\.


--
-- Data for Name: pilot; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.pilot (id, name, lizenz) FROM stdin;
1	Pia	ATPL-1
\.


--
-- Data for Name: rolle; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.rolle (id, bezeichnung) FROM stdin;
1	Kapitän
2	Erster Offizier
\.


--
-- Data for Name: sitzplatz; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.sitzplatz (flugzeug_id, reihe, buchstabe, klasse) FROM stdin;
1	12	A	economy
1	12	B	economy
\.


--
-- Data for Name: ticket; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.ticket (id, status, sitzplatz, kunde_id, flug_id) FROM stdin;
1	bezahlt	12A	1	1
2	reserviert	\N	2	2
\.


--
-- Data for Name: vielflieger; Type: TABLE DATA; Schema: public; Owner: postgres
--

COPY public.vielflieger (kunde_id, meilen, stufe) FROM stdin;
1	12000	basis
\.


--
-- Name: Container_ID_seq; Type: SEQUENCE SET; Schema: fracht; Owner: postgres
--

SELECT pg_catalog.setval('fracht."Container_ID_seq"', 1, true);


--
-- Name: extra_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.extra_id_seq', 1, true);


--
-- Name: flug_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.flug_id_seq', 2, true);


--
-- Name: fluggesellschaft_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.fluggesellschaft_id_seq', 2, true);


--
-- Name: flugzeug_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.flugzeug_id_seq', 2, true);


--
-- Name: kunde_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.kunde_id_seq', 2, true);


--
-- Name: kunde_konto_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.kunde_konto_id_seq', 1, true);


--
-- Name: mitarbeiter_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.mitarbeiter_id_seq', 2, true);


--
-- Name: pilot_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.pilot_id_seq', 1, true);


--
-- Name: rolle_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.rolle_id_seq', 2, true);


--
-- Name: ticket_id_seq; Type: SEQUENCE SET; Schema: public; Owner: postgres
--

SELECT pg_catalog.setval('public.ticket_id_seq', 2, true);


--
-- Name: Container Container_pkey; Type: CONSTRAINT; Schema: fracht; Owner: postgres
--

ALTER TABLE ONLY fracht."Container"
    ADD CONSTRAINT "Container_pkey" PRIMARY KEY ("ID");


--
-- Name: buchung_extra buchung_extra_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.buchung_extra
    ADD CONSTRAINT buchung_extra_pkey PRIMARY KEY (ticket_id, extra_id);


--
-- Name: einsatz einsatz_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.einsatz
    ADD CONSTRAINT einsatz_pkey PRIMARY KEY (pilot_id, flug_id, rolle_id);


--
-- Name: extra extra_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.extra
    ADD CONSTRAINT extra_pkey PRIMARY KEY (id);


--
-- Name: flug_crew flug_crew_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug_crew
    ADD CONSTRAINT flug_crew_pkey PRIMARY KEY (flug_id, mitarbeiter_id);


--
-- Name: flug flug_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug
    ADD CONSTRAINT flug_pkey PRIMARY KEY (id);


--
-- Name: fluggesellschaft fluggesellschaft_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.fluggesellschaft
    ADD CONSTRAINT fluggesellschaft_pkey PRIMARY KEY (id);


--
-- Name: flughafen flughafen_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flughafen
    ADD CONSTRAINT flughafen_pkey PRIMARY KEY (id);


--
-- Name: flugzeug flugzeug_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flugzeug
    ADD CONSTRAINT flugzeug_pkey PRIMARY KEY (id);


--
-- Name: gate gate_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.gate
    ADD CONSTRAINT gate_pkey PRIMARY KEY (flughafen_id, nummer);


--
-- Name: kunde kunde_email_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.kunde
    ADD CONSTRAINT kunde_email_key UNIQUE (email);


--
-- Name: kunde_konto kunde_konto_kunde_id_key; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.kunde_konto
    ADD CONSTRAINT kunde_konto_kunde_id_key UNIQUE (kunde_id);


--
-- Name: kunde_konto kunde_konto_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.kunde_konto
    ADD CONSTRAINT kunde_konto_pkey PRIMARY KEY (id);


--
-- Name: kunde kunde_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.kunde
    ADD CONSTRAINT kunde_pkey PRIMARY KEY (id);


--
-- Name: mitarbeiter mitarbeiter_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.mitarbeiter
    ADD CONSTRAINT mitarbeiter_pkey PRIMARY KEY (id);


--
-- Name: pilot pilot_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.pilot
    ADD CONSTRAINT pilot_pkey PRIMARY KEY (id);


--
-- Name: rolle rolle_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.rolle
    ADD CONSTRAINT rolle_pkey PRIMARY KEY (id);


--
-- Name: sitzplatz sitzplatz_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sitzplatz
    ADD CONSTRAINT sitzplatz_pkey PRIMARY KEY (flugzeug_id, reihe, buchstabe);


--
-- Name: ticket ticket_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.ticket
    ADD CONSTRAINT ticket_pkey PRIMARY KEY (id);


--
-- Name: vielflieger vielflieger_pkey; Type: CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.vielflieger
    ADD CONSTRAINT vielflieger_pkey PRIMARY KEY (kunde_id);


--
-- Name: ix_flug_start; Type: INDEX; Schema: public; Owner: postgres
--

CREATE INDEX ix_flug_start ON public.flug USING btree (start_flughafen_id, start_zeit);


--
-- Name: ticket ticket_storno; Type: TRIGGER; Schema: public; Owner: postgres
--

CREATE TRIGGER ticket_storno BEFORE UPDATE ON public.ticket FOR EACH ROW EXECUTE FUNCTION public.ticket_storno();


--
-- Name: Container Container_flug_id_fkey; Type: FK CONSTRAINT; Schema: fracht; Owner: postgres
--

ALTER TABLE ONLY fracht."Container"
    ADD CONSTRAINT "Container_flug_id_fkey" FOREIGN KEY (flug_id) REFERENCES public.flug(id);


--
-- Name: Fracht Stück Fracht Stück_kunde_email_fkey; Type: FK CONSTRAINT; Schema: fracht; Owner: postgres
--

ALTER TABLE ONLY fracht."Fracht Stück"
    ADD CONSTRAINT "Fracht Stück_kunde_email_fkey" FOREIGN KEY (kunde_email) REFERENCES public.kunde(email);


--
-- Name: buchung_extra buchung_extra_extra_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.buchung_extra
    ADD CONSTRAINT buchung_extra_extra_id_fkey FOREIGN KEY (extra_id) REFERENCES public.extra(id);


--
-- Name: buchung_extra buchung_extra_ticket_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.buchung_extra
    ADD CONSTRAINT buchung_extra_ticket_id_fkey FOREIGN KEY (ticket_id) REFERENCES public.ticket(id);


--
-- Name: einsatz einsatz_flug_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.einsatz
    ADD CONSTRAINT einsatz_flug_id_fkey FOREIGN KEY (flug_id) REFERENCES public.flug(id);


--
-- Name: einsatz einsatz_pilot_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.einsatz
    ADD CONSTRAINT einsatz_pilot_id_fkey FOREIGN KEY (pilot_id) REFERENCES public.pilot(id);


--
-- Name: einsatz einsatz_rolle_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.einsatz
    ADD CONSTRAINT einsatz_rolle_id_fkey FOREIGN KEY (rolle_id) REFERENCES public.rolle(id);


--
-- Name: flug fk_flug_flugzeug; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug
    ADD CONSTRAINT fk_flug_flugzeug FOREIGN KEY (flugzeug_id) REFERENCES public.flugzeug(id);


--
-- Name: flug fk_flug_gate; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug
    ADD CONSTRAINT fk_flug_gate FOREIGN KEY (gate_flughafen_id, gate_nummer) REFERENCES public.gate(flughafen_id, nummer) ON DELETE SET NULL;


--
-- Name: flug fk_flug_start; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug
    ADD CONSTRAINT fk_flug_start FOREIGN KEY (start_flughafen_id) REFERENCES public.flughafen(id);


--
-- Name: flug fk_flug_ziel; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug
    ADD CONSTRAINT fk_flug_ziel FOREIGN KEY (ziel_flughafen_id) REFERENCES public.flughafen(id);


--
-- Name: ticket fk_ticket_flug; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.ticket
    ADD CONSTRAINT fk_ticket_flug FOREIGN KEY (flug_id) REFERENCES public.flug(id);


--
-- Name: flug_crew flug_crew_flug_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug_crew
    ADD CONSTRAINT flug_crew_flug_id_fkey FOREIGN KEY (flug_id) REFERENCES public.flug(id);


--
-- Name: flug_crew flug_crew_mitarbeiter_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flug_crew
    ADD CONSTRAINT flug_crew_mitarbeiter_id_fkey FOREIGN KEY (mitarbeiter_id) REFERENCES public.mitarbeiter(id);


--
-- Name: flugzeug flugzeug_fluggesellschaft_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.flugzeug
    ADD CONSTRAINT flugzeug_fluggesellschaft_id_fkey FOREIGN KEY (fluggesellschaft_id) REFERENCES public.fluggesellschaft(id);


--
-- Name: gate gate_flughafen_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.gate
    ADD CONSTRAINT gate_flughafen_id_fkey FOREIGN KEY (flughafen_id) REFERENCES public.flughafen(id);


--
-- Name: kunde_konto kunde_konto_kunde_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.kunde_konto
    ADD CONSTRAINT kunde_konto_kunde_id_fkey FOREIGN KEY (kunde_id) REFERENCES public.kunde(id);


--
-- Name: mitarbeiter mitarbeiter_vorgesetzter_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.mitarbeiter
    ADD CONSTRAINT mitarbeiter_vorgesetzter_id_fkey FOREIGN KEY (vorgesetzter_id) REFERENCES public.mitarbeiter(id);


--
-- Name: sitzplatz sitzplatz_flugzeug_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.sitzplatz
    ADD CONSTRAINT sitzplatz_flugzeug_id_fkey FOREIGN KEY (flugzeug_id) REFERENCES public.flugzeug(id) ON DELETE CASCADE;


--
-- Name: ticket ticket_kunde_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.ticket
    ADD CONSTRAINT ticket_kunde_id_fkey FOREIGN KEY (kunde_id) REFERENCES public.kunde(id) ON DELETE CASCADE;


--
-- Name: vielflieger vielflieger_kunde_id_fkey; Type: FK CONSTRAINT; Schema: public; Owner: postgres
--

ALTER TABLE ONLY public.vielflieger
    ADD CONSTRAINT vielflieger_kunde_id_fkey FOREIGN KEY (kunde_id) REFERENCES public.kunde(id);


--
-- PostgreSQL database dump complete
--

\unrestrict KEY

