-- MySQL dump 10.13  Distrib 9.7.4, for Linux (x86_64)
--
-- Host: 127.0.0.1    Database: flug
-- ------------------------------------------------------
-- Server version	9.7.4

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET @OLD_CHARACTER_SET_RESULTS=@@CHARACTER_SET_RESULTS */;
/*!40101 SET @OLD_COLLATION_CONNECTION=@@COLLATION_CONNECTION */;
/*!50503 SET NAMES utf8mb4 */;
/*!40103 SET @OLD_TIME_ZONE=@@TIME_ZONE */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;
/*!40014 SET @OLD_FOREIGN_KEY_CHECKS=@@FOREIGN_KEY_CHECKS, FOREIGN_KEY_CHECKS=0 */;
/*!40101 SET @OLD_SQL_MODE=@@SQL_MODE, SQL_MODE='NO_AUTO_VALUE_ON_ZERO' */;
/*!40111 SET @OLD_SQL_NOTES=@@SQL_NOTES, SQL_NOTES=0 */;
SET @MYSQLDUMP_TEMP_LOG_BIN = @@SESSION.SQL_LOG_BIN;
SET @@SESSION.SQL_LOG_BIN= 0;

--
-- GTID state at the beginning of the backup 
--

SET @@GLOBAL.GTID_PURGED=/*!80000 '+'*/ '2a962c0b-d56d-11f1-993f-d2586ea17ed1:1-47';

--
-- Table structure for table `Fracht Stück`
--

DROP TABLE IF EXISTS `Fracht Stück`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `Fracht Stück` (
  `Sendungs Nr` int NOT NULL,
  `Gewicht kg` decimal(7,2) NOT NULL,
  `flug_nummer` varchar(8) DEFAULT NULL,
  `kunde_email` varchar(120) DEFAULT NULL,
  KEY `fk_fracht_kunde` (`kunde_email`),
  CONSTRAINT `fk_fracht_kunde` FOREIGN KEY (`kunde_email`) REFERENCES `kunde` (`email`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='Cargo item; no primary key on purpose';
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `buchung_extra`
--

DROP TABLE IF EXISTS `buchung_extra`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `buchung_extra` (
  `ticket_id` int NOT NULL,
  `extra_id` int NOT NULL,
  `anzahl` smallint NOT NULL DEFAULT '1',
  `preis` decimal(8,2) NOT NULL,
  PRIMARY KEY (`ticket_id`,`extra_id`),
  KEY `extra_id` (`extra_id`),
  CONSTRAINT `buchung_extra_ibfk_1` FOREIGN KEY (`ticket_id`) REFERENCES `ticket` (`id`),
  CONSTRAINT `buchung_extra_ibfk_2` FOREIGN KEY (`extra_id`) REFERENCES `extra` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `einsatz`
--

DROP TABLE IF EXISTS `einsatz`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `einsatz` (
  `pilot_id` int NOT NULL,
  `flug_id` int NOT NULL,
  `rolle_id` int NOT NULL,
  `stunden` decimal(4,1) DEFAULT NULL,
  PRIMARY KEY (`pilot_id`,`flug_id`,`rolle_id`),
  KEY `fk_einsatz_flug` (`flug_id`),
  KEY `fk_einsatz_rolle` (`rolle_id`),
  CONSTRAINT `fk_einsatz_flug` FOREIGN KEY (`flug_id`) REFERENCES `flug` (`id`),
  CONSTRAINT `fk_einsatz_pilot` FOREIGN KEY (`pilot_id`) REFERENCES `pilot` (`id`),
  CONSTRAINT `fk_einsatz_rolle` FOREIGN KEY (`rolle_id`) REFERENCES `rolle` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `extra`
--

DROP TABLE IF EXISTS `extra`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `extra` (
  `id` int NOT NULL AUTO_INCREMENT,
  `bezeichnung` varchar(60) NOT NULL,
  `preis` decimal(8,2) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `flug`
--

DROP TABLE IF EXISTS `flug`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `flug` (
  `id` int NOT NULL AUTO_INCREMENT,
  `nummer` varchar(8) NOT NULL,
  `preis` decimal(8,2) NOT NULL DEFAULT '0.00',
  `start_zeit` datetime NOT NULL,
  `end_zeit` datetime NOT NULL,
  `dauer_min` int GENERATED ALWAYS AS (timestampdiff(MINUTE,`start_zeit`,`end_zeit`)) VIRTUAL,
  `flugzeug_id` int NOT NULL,
  `start_flughafen_id` char(3) NOT NULL,
  `ziel_flughafen_id` char(3) NOT NULL,
  `gate_flughafen_id` char(3) DEFAULT NULL,
  `gate_nummer` varchar(5) DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `ix_flug_start` (`start_flughafen_id`,`start_zeit`),
  KEY `fk_flug_flugzeug` (`flugzeug_id`),
  KEY `fk_flug_ziel` (`ziel_flughafen_id`),
  KEY `fk_flug_gate` (`gate_flughafen_id`,`gate_nummer`),
  CONSTRAINT `fk_flug_flugzeug` FOREIGN KEY (`flugzeug_id`) REFERENCES `flugzeug` (`id`),
  CONSTRAINT `fk_flug_gate` FOREIGN KEY (`gate_flughafen_id`, `gate_nummer`) REFERENCES `gate` (`flughafen_id`, `nummer`) ON DELETE SET NULL,
  CONSTRAINT `fk_flug_start` FOREIGN KEY (`start_flughafen_id`) REFERENCES `flughafen` (`id`),
  CONSTRAINT `fk_flug_ziel` FOREIGN KEY (`ziel_flughafen_id`) REFERENCES `flughafen` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `flug_crew`
--

DROP TABLE IF EXISTS `flug_crew`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `flug_crew` (
  `flug_id` int NOT NULL,
  `mitarbeiter_id` int NOT NULL,
  PRIMARY KEY (`flug_id`,`mitarbeiter_id`),
  KEY `fk_crew_mitarbeiter` (`mitarbeiter_id`),
  CONSTRAINT `fk_crew_flug` FOREIGN KEY (`flug_id`) REFERENCES `flug` (`id`),
  CONSTRAINT `fk_crew_mitarbeiter` FOREIGN KEY (`mitarbeiter_id`) REFERENCES `mitarbeiter` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `fluggesellschaft`
--

DROP TABLE IF EXISTS `fluggesellschaft`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `fluggesellschaft` (
  `id` int NOT NULL AUTO_INCREMENT,
  `name` varchar(100) NOT NULL,
  `adresse` varchar(200) DEFAULT NULL,
  `telefon` varchar(30) DEFAULT NULL,
  `email` varchar(120) DEFAULT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `flughafen`
--

DROP TABLE IF EXISTS `flughafen`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `flughafen` (
  `id` char(3) NOT NULL COMMENT 'IATA code',
  `name` varchar(100) NOT NULL,
  `stadt` varchar(80) NOT NULL,
  `land` varchar(80) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Temporary view structure for view `flugplan`
--

DROP TABLE IF EXISTS `flugplan`;
/*!50001 DROP VIEW IF EXISTS `flugplan`*/;
SET @saved_cs_client     = @@character_set_client;
/*!50503 SET character_set_client = utf8mb4 */;
/*!50001 CREATE VIEW `flugplan` AS SELECT 
 1 AS `nummer`,
 1 AS `von`,
 1 AS `nach`,
 1 AS `start_zeit`*/;
SET character_set_client = @saved_cs_client;

--
-- Table structure for table `flugzeug`
--

DROP TABLE IF EXISTS `flugzeug`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `flugzeug` (
  `id` int NOT NULL AUTO_INCREMENT,
  `modell` varchar(60) NOT NULL,
  `sitze` smallint unsigned NOT NULL,
  `geschwindigkeit` decimal(6,1) DEFAULT NULL COMMENT 'cruise speed in km/h',
  `fluggesellschaft_id` int NOT NULL,
  PRIMARY KEY (`id`),
  KEY `fk_flugzeug_gesellschaft` (`fluggesellschaft_id`),
  CONSTRAINT `fk_flugzeug_gesellschaft` FOREIGN KEY (`fluggesellschaft_id`) REFERENCES `fluggesellschaft` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `gate`
--

DROP TABLE IF EXISTS `gate`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `gate` (
  `flughafen_id` char(3) NOT NULL,
  `nummer` varchar(5) NOT NULL,
  `terminal` varchar(5) DEFAULT NULL,
  PRIMARY KEY (`flughafen_id`,`nummer`),
  CONSTRAINT `fk_gate_flughafen` FOREIGN KEY (`flughafen_id`) REFERENCES `flughafen` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `kunde`
--

DROP TABLE IF EXISTS `kunde`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `kunde` (
  `id` int NOT NULL AUTO_INCREMENT,
  `vorname` varchar(50) NOT NULL,
  `nachname` varchar(50) NOT NULL,
  `adresse` varchar(200) DEFAULT NULL,
  `email` varchar(120) NOT NULL COMMENT 'login address, unique',
  `telefon` varchar(30) DEFAULT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uq_kunde_email` (`email`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci COMMENT='Customer who books tickets';
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `kunde_konto`
--

DROP TABLE IF EXISTS `kunde_konto`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `kunde_konto` (
  `id` int NOT NULL AUTO_INCREMENT,
  `kunde_id` int NOT NULL,
  `login` varchar(40) NOT NULL,
  `passwort_hash` varbinary(64) NOT NULL,
  PRIMARY KEY (`id`),
  UNIQUE KEY `uq_konto_kunde` (`kunde_id`),
  CONSTRAINT `fk_konto_kunde` FOREIGN KEY (`kunde_id`) REFERENCES `kunde` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `mitarbeiter`
--

DROP TABLE IF EXISTS `mitarbeiter`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `mitarbeiter` (
  `id` int NOT NULL AUTO_INCREMENT,
  `name` varchar(100) NOT NULL,
  `vorgesetzter_id` int DEFAULT NULL,
  PRIMARY KEY (`id`),
  KEY `fk_mitarbeiter_chef` (`vorgesetzter_id`),
  CONSTRAINT `fk_mitarbeiter_chef` FOREIGN KEY (`vorgesetzter_id`) REFERENCES `mitarbeiter` (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `pilot`
--

DROP TABLE IF EXISTS `pilot`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `pilot` (
  `id` int NOT NULL AUTO_INCREMENT,
  `name` varchar(100) NOT NULL,
  `lizenz` varchar(20) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=2 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `rolle`
--

DROP TABLE IF EXISTS `rolle`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `rolle` (
  `id` int NOT NULL AUTO_INCREMENT,
  `bezeichnung` varchar(40) NOT NULL,
  PRIMARY KEY (`id`)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `sitzplatz`
--

DROP TABLE IF EXISTS `sitzplatz`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `sitzplatz` (
  `flugzeug_id` int NOT NULL,
  `reihe` tinyint unsigned NOT NULL,
  `buchstabe` char(1) NOT NULL,
  `klasse` enum('economy','business','first') NOT NULL DEFAULT 'economy',
  PRIMARY KEY (`flugzeug_id`,`reihe`,`buchstabe`),
  CONSTRAINT `fk_sitzplatz_flugzeug` FOREIGN KEY (`flugzeug_id`) REFERENCES `flugzeug` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Table structure for table `ticket`
--

DROP TABLE IF EXISTS `ticket`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `ticket` (
  `id` int NOT NULL AUTO_INCREMENT,
  `status` enum('reserviert','bezahlt','storniert') NOT NULL DEFAULT 'reserviert',
  `sitzplatz` varchar(4) DEFAULT NULL,
  `kunde_id` int NOT NULL,
  `flug_id` int NOT NULL,
  PRIMARY KEY (`id`),
  KEY `fk_ticket_kunde` (`kunde_id`),
  KEY `fk_ticket_flug` (`flug_id`),
  CONSTRAINT `fk_ticket_flug` FOREIGN KEY (`flug_id`) REFERENCES `flug` (`id`),
  CONSTRAINT `fk_ticket_kunde` FOREIGN KEY (`kunde_id`) REFERENCES `kunde` (`id`) ON DELETE CASCADE
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;
/*!50003 SET @saved_cs_client      = @@character_set_client */ ;
/*!50003 SET @saved_cs_results     = @@character_set_results */ ;
/*!50003 SET @saved_col_connection = @@collation_connection */ ;
/*!50003 SET character_set_client  = utf8mb4 */ ;
/*!50003 SET character_set_results = utf8mb4 */ ;
/*!50003 SET collation_connection  = utf8mb4_0900_ai_ci */ ;
/*!50003 SET @saved_sql_mode       = @@sql_mode */ ;
/*!50003 SET sql_mode              = 'ONLY_FULL_GROUP_BY,STRICT_TRANS_TABLES,NO_ZERO_IN_DATE,NO_ZERO_DATE,ERROR_FOR_DIVISION_BY_ZERO,NO_ENGINE_SUBSTITUTION' */ ;
DELIMITER ;;
/*!50003 CREATE*/ /*!50017 DEFINER=`root`@`%`*/ /*!50003 TRIGGER `ticket_storno` BEFORE UPDATE ON `ticket` FOR EACH ROW BEGIN
    IF NEW.status = 'storniert' THEN
        SET NEW.sitzplatz = NULL;
    END IF;
END */;;
DELIMITER ;
/*!50003 SET sql_mode              = @saved_sql_mode */ ;
/*!50003 SET character_set_client  = @saved_cs_client */ ;
/*!50003 SET character_set_results = @saved_cs_results */ ;
/*!50003 SET collation_connection  = @saved_col_connection */ ;

--
-- Table structure for table `vielflieger`
--

DROP TABLE IF EXISTS `vielflieger`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!50503 SET character_set_client = utf8mb4 */;
CREATE TABLE `vielflieger` (
  `kunde_id` int NOT NULL,
  `meilen` int NOT NULL DEFAULT '0',
  `stufe` varchar(10) NOT NULL DEFAULT 'basis',
  PRIMARY KEY (`kunde_id`),
  CONSTRAINT `fk_vielflieger_kunde` FOREIGN KEY (`kunde_id`) REFERENCES `kunde` (`id`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_0900_ai_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

--
-- Final view structure for view `flugplan`
--

/*!50001 DROP VIEW IF EXISTS `flugplan`*/;
/*!50001 SET @saved_cs_client          = @@character_set_client */;
/*!50001 SET @saved_cs_results         = @@character_set_results */;
/*!50001 SET @saved_col_connection     = @@collation_connection */;
/*!50001 SET character_set_client      = utf8mb4 */;
/*!50001 SET character_set_results     = utf8mb4 */;
/*!50001 SET collation_connection      = utf8mb4_0900_ai_ci */;
/*!50001 CREATE ALGORITHM=UNDEFINED */
/*!50013 DEFINER=`root`@`%` SQL SECURITY DEFINER */
/*!50001 VIEW `flugplan` AS select `f`.`nummer` AS `nummer`,`f`.`start_flughafen_id` AS `von`,`f`.`ziel_flughafen_id` AS `nach`,`f`.`start_zeit` AS `start_zeit` from `flug` `f` */;
/*!50001 SET character_set_client      = @saved_cs_client */;
/*!50001 SET character_set_results     = @saved_cs_results */;
/*!50001 SET collation_connection      = @saved_col_connection */;
SET @@SESSION.SQL_LOG_BIN = @MYSQLDUMP_TEMP_LOG_BIN;
/*!40103 SET TIME_ZONE=@OLD_TIME_ZONE */;

/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;
/*!40014 SET FOREIGN_KEY_CHECKS=@OLD_FOREIGN_KEY_CHECKS */;
/*!40014 SET UNIQUE_CHECKS=@OLD_UNIQUE_CHECKS */;
/*!40101 SET CHARACTER_SET_CLIENT=@OLD_CHARACTER_SET_CLIENT */;
/*!40101 SET CHARACTER_SET_RESULTS=@OLD_CHARACTER_SET_RESULTS */;
/*!40101 SET COLLATION_CONNECTION=@OLD_COLLATION_CONNECTION */;
/*!40111 SET SQL_NOTES=@OLD_SQL_NOTES */;

-- Dump completed
