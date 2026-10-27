//! Concordant Fabric-101 release metadata, pinned to the authors' 2026-10-26 data revision.
//! Numerical observations only: no upstream code, raw curve archive, or cylinder-to-sheet law.

use std::collections::BTreeMap;

use crate::materials::{Component, Evidence, Form, Material, Quantity, Source};

const REVISION: &str = "510519be0657a341a1a84fdb5c24be156ac83c9f";
const REPOSITORY: &str = "https://github.com/GongDeshan/Fabric_101_for_Wrinkles";
const ARTICLE: &str = "https://arxiv.org/html/2609.13707v1";
const CATALOG_SHA256: &str = "6a460054c3031554733c7706a8327c89e323ff4a4657352f5bcec0494d7cf96a";
const MAPPING_SHA256: &str = "57d7e0225b37ee7ca556b91f1e20d0d89450dd1927f3992ac467a42b4d36933c";
const CONDITIONING: &str = "Paper section 3.1: 24 h at 20 +/- 2 degrees C and 65 +/- 5 percent RH, ISO 139:2005; exact per-run moisture content and conditioning log not released";

// Release ID, name, category, g/m2, mm, exact metadata path, Git blob SHA-1, instrument ID, CSV line.
// Each retained row agrees with its own instrument header within the header's printed precision.
// Full CSV precision is retained, not promoted to measurement accuracy or an uncertainty interval.
struct Row(
    &'static str,
    &'static str,
    &'static str,
    f64,
    f64,
    &'static str,
    &'static str,
    &'static str,
    usize,
);

#[rustfmt::skip]
const ROWS: &[Row] = &[
    Row("C01", "Cotton Plain", "Cotton", 134.84613212874083, 0.7, "C01/002/head.dat", "13a8fd12b76e43d9becb77bc5e7fe5a101e5987d", "110_C01_002", 2),
    Row("C02", "Cotton Poplin", "Cotton", 108.06515693285658, 0.2, "C02/001/head.dat", "a4319edaa6d1648d13d8387aac0d3211a3ecf085", "111_C02_001", 3),
    Row("C03", "Cotton Muslin", "Cotton", 76.4618774153935, 0.3, "C03/001/head.dat", "d37b4f18861ad97523b6c9a34dacb391380ce36e", "112_C03_001", 4),
    Row("C04", "Cotton Oxford", "Cotton", 156.93404634581105, 0.5, "C04/001/head.dat", "3e6400d2a359462342566732481b11c65b9c1a5d", "113_C04_001", 5),
    Row("C05", "Cotton Chambray", "Cotton", 144.69226425748164, 0.7, "C05/001/head.dat", "24fb708bd0c695acc56cd87cf30afcb32846e38e", "114_C05_001", 6),
    Row("C06", "Cotton Twill", "Cotton", 115.72192513368985, 0.6, "C06/001/head.dat", "2e3f5e108405de8db0df05a725ee779167b1872a", "1004_C06_001", 7),
    Row("C07", "Cotton Khaki", "Cotton", 228.4848484848485, 0.6, "C07/001/head.dat", "dda4e18e7b9255aef74985cfabc6c56710e5e20f", "116_C07_001", 8),
    Row("C08", "Cotton Canvas", "Cotton", 238.50267379679147, 0.6, "C08/001/head.dat", "6639c499ea2b2e3b3fce5e7f51a03d5c77d6d62c", "117_C08_001", 9),
    Row("C09", "Cotton Corduroy", "Cotton", 297.1578947368421, 1.5, "C09/001/head.dat", "784839022bd9949e2adf26d5d922d485ea34dabd", "118_C09_001", 10),
    Row("C10", "Cotton Velveteen", "Cotton", 229.1578947368421, 1.5, "C10/001/head.dat", "e9e046ac8edb2428eac44f26f136ee7008320d88", "119_C10_001", 11),
    Row("C11", "Classic Denim", "Cotton", 293.89473684210526, 1.0, "C11/001/head.dat", "2f98c71639388914621aeccc3d5bec594700c87e", "120_C11_001", 12),
    Row("C12", "Stretch Denim", "Cotton", 308.4848484848485, 0.6, "C12/001/head.dat", "3137d76afd353c253b2ba7864d9ef88bf492e41d", "121_C12_001", 13),
    Row("C13", "Lightweight Denim", "Cotton", 249.68238283455676, 0.7, "C13/001/head.dat", "a05275c660d3239bc5a2c541e8f744a4575bb64a", "122_C13_001", 14),
    Row("Drape 1", "Muslin Natural", "Cotton", 60.89500487593455, 0.3, "Drape 1/001/head.dat", "f6142c3713d1ad8b9fa9b9a7435eb5873e95469c", "187_Drape 1_001", 15),
    Row("Drape 2", "Blue Cotton", "Cotton", 192.86420101637492, 0.7, "Drape 2/001/head.dat", "1ed4f088d10c26ce6b2024fd674b0cb3857c1da5", "188_Drape 2_001", 16),
    Row("Drape 9", "Cotton Canvas", "Cotton", 249.75296442687747, 0.7, "Drape 9/001/head.dat", "1bf864b68f44301c29fea43369d4df56adb9dfc7", "195_Drape 9_001", 18),
    Row("Drape 11", "Brushed Cotton Check", "Cotton", 162.33766233766232, 0.9, "Drape 11/001/head.dat", "195a893882deb462244322a2184842b03e3558d0", "197_Drape 11_001", 19),
    Row("E-C01", "High-count Cotton Poplin", "Cotton", 84.26337270199011, 0.4, "E-C01/001/head.dat", "c304ee80dc98714bc3b69ed9b52d26e580edde74", "152_E-C01_001", 20),
    Row("E-C02", "Low-count Cotton Canvas", "Cotton", 576.4473684210527, 2.3, "E-C02/001/head.dat", "70bc1c70f17961ed44d449e8283c0db354a5d8b3", "1027_EC02_001", 21),
    Row("E-C03", "High-density Cotton Plain", "Cotton", 101.88897316430094, 0.2, "E-C03/001/head.dat", "82fbb622c57fec8cdff5e4621f8e73f7abd43cbe", "203_E-C03_001", 22),
    Row("E-C04", "Stretch Cotton Khaki", "Cotton", 246.55971479500894, 0.6, "E-C04/001/head.dat", "78ae4f68c78b9a429fe4716796337ec217eecea8", "154_E-C04_001", 23),
    Row("E-C07", "Mercerized Cotton Plain", "Cotton", 123.37920323617583, 0.4, "E-C07/001/head.dat", "93e692be44f8f88631011e549e3a8601ca81c962", "157_E-C07_001", 24),
    Row("E-C08", "Wrinkle-free Cotton Poplin", "Cotton", 109.50987828222631, 0.3, "E-C08/001/head.dat", "e199d8813d63552a9d276f6ca037e617605d4c43", "158_E-C08_001", 25),
    Row("E-C09", "Brushed Cotton Twill", "Cotton", 219.75578769057032, 0.8, "E-C09/001/head.dat", "885c0cfb41cdbc8a18509182826e2be7b523e7a4", "159_E-C09_001", 26),
    Row("E-C10", "Cotton Herringbone Twill", "Cotton", 246.4736842105263, 1.0, "E-C10/001/head.dat", "1cef607f01a2b458ebc48ece7acae120860f8ba5", "160_E-C10_001", 27),
    Row("E-KN01", "Combed Cotton Jersey", "Cotton", 108.76623376623377, 0.7, "E-KN01/001/head.dat", "35ff23c80e0c09d0fe32400577a270f1536dc303", "176_E-KN01_001", 28),
    Row("E-KN03", "Cotton Interlock", "Cotton", 323.10526315789474, 1.6, "E-KN03/001/head.dat", "d22a2af3c04f5de3dd9a435a7a1187b025ac2b72", "177_E-KN03_001", 29),
    Row("E-KN04", "Cotton Air Layer", "Cotton", 291.3157894736842, 1.4, "E-KN04/001/head.dat", "a408297d75978fb0086cc45f84f5efab6ef587ae", "178_E-KN04_001", 30),
    Row("E-KN05", "Cotton Terry Cloth", "Cotton", 273.10526315789474, 3.0, "E-KN05/001/head.dat", "8bfb9f64f21af9fc7734b03a79d4a7d274f502b4", "179_E-KN05_001", 31),
    Row("KN01", "Cotton Jersey", "Cotton", 174.33653303218523, 0.7, "KN01/001/head.dat", "acede530faf25c123b3abad8d7028d668a8a3831", "149_KN01_001", 33),
    Row("KN02", "Cotton Rib", "Cotton", 309.959062676454, 0.9, "KN02/001/head.dat", "1fdf2ca9f1de788acfe023159b790ad894a041ca", "150_KN02_001", 34),
    Row("KN03", "Cotton French Terry", "Cotton", 239.60526315789474, 1.4, "KN03/001/head.dat", "0ea25c350bd64a0839bef1b613a0d159dbf41ca4", "151_KN03_001", 35),
    Row("Drape 8", "Washed Linen Aqua", "Linen and ramie", 240.25974025974025, 0.8, "Drape 8/001/head.dat", "b7f937bf8503d6298fa2e251adbbac03773c6ac0", "194_Drape 8_001", 36),
    Row("L01", "Linen Plain", "Linen and ramie", 117.3836096362914, 0.4, "L01/001/head.dat", "02872ba3be75fd3e3f3a01367289b8314ecba93b", "200_L01_001", 37),
    Row("L02", "Dew Retting Linen", "Linen and ramie", 136.96969696969697, 0.5, "L02/001/head.dat", "9405b218eb052cab1bb8f5f9dc0875ce2b29f937", "124_L02_001", 38),
    Row("L03", "Ramie Plain", "Linen and ramie", 65.77093943005742, 0.3, "L03/001/head.dat", "b34aa1cdb8a1b70869547402fdbad32bbd1714c0", "125_L03_001", 39),
    Row("Drape 5", "Tartan Wool Fabrics", "Wool", 114.55392433653303, 0.8, "Drape 5/001/head.dat", "1db45ece02fb46b46b3550a6d29b35ee36fef0ef", "191_Drape 5_001", 40),
    Row("E-W03", "Hard-twist Worsted Valitin", "Wool", 184.1297359771734, 0.4, "E-W03/001/head.dat", "dca51a99238e2e2a168f97fdaba918cc1c328b2f", "164_E-W03_001", 41),
    Row("E-W04", "Machine-washable Worsted", "Wool", 338.18421052631584, 1.8, "E-W04/001/head.dat", "44f50c4b4cf5faf9d05bc1c261860a48fb85ac79", "165_E-W04_001", 42),
    Row("W01", "Worsted Gabardine", "Wool", 191.89511323003575, 0.4, "W01/001/head.dat", "54c5c0f98941b0c12776374a39c8aaece9e9068d", "126_W01_001", 43),
    Row("W02", "Worsted Valitin", "Wool", 172.06631270993608, 0.3, "W02/001/head.dat", "d3acf87b2743ae9021b3246cef1d8253f18aeca5", "127_W02_001", 44),
    Row("W03", "Woolen Flannel", "Wool", 268.9511575381141, 0.7, "W03/001/head.dat", "ecf8f232387c3a8ecb984ba9096154f9e94eea16", "128_W03_001", 45),
    Row("W04", "Woolen Tweed", "Wool", 249.68238283455676, 0.7, "W04/001/head.dat", "3ad02868b5f1c7d5a98a9d05e68ce8b9397ef958", "129_W04_001", 46),
    Row("E-S01", "Heavy Silk Crepe", "Silk", 142.81070538519884, 0.4, "E-S01/001/head.dat", "959391d72f4c4b3a3b102a805cbb51ac691cf704", "1017_ES01_001", 47),
    Row("E-S02", "Silk Crepe Georgette", "Silk", 19.03420377794633, 0.2, "E-S02/001/head.dat", "b35dba190d16bfccfe4dacf14653d9ea7a237da5", "1003_ES02_001", 48),
    Row("E-S03", "Silk Douppioni", "Silk", 86.93610719832412, 0.3, "E-S03/001/head.dat", "2e38f89b7b36a76ef190eecf0aba60d223deeef1", "168_E-S03_001", 49),
    Row("E-S04", "Silk Burn-out", "Silk", 53.1296276230722, 0.3, "E-S04/001/head.dat", "c41e05720fa046e8f234c696f1d5491fb4a064c7", "1008_ES04_001", 50),
    Row("S01", "Silk Satin", "Silk", 72.66948387329793, 0.2, "S01/001/head.dat", "f326a690403bbd2f7e75fb4b79f30e1c631fb3c7", "1018_S01_001", 51),
    Row("S02", "Silk Crepe de Chine", "Silk", 92.02874995485246, 0.3, "S02/001/head.dat", "5bbb75b3575969d27b8300263a8e2b37c59f982a", "1006_S02_001", 52),
    Row("S03", "Silk Habotai", "Silk", 52.44338498212157, 0.1, "S03/001/head.dat", "fa2f68fa182527db2b3071197a656004dee66dc5", "132_S03_001", 53),
    Row("S04", "Silk Georgette", "Silk", 45.11142413407015, 0.2, "S04/001/head.dat", "8e26acd008461c9b99efb4f662013dc3df4c11e9", "1007_S04_001", 54),
    Row("S05", "Silk Chiffon", "Silk", 25.390977715173186, 0.1, "S05/001/head.dat", "7f38bc732b5493ae35145b5d1889d23a523907d3", "1009_S05_001", 55),
    Row("S06", "Silk Taffeta", "Silk", 67.6490771842381, 0.1, "S06/001/head.dat", "ef984ff1863993a5cce443d3db6be7d8e9275709", "1024_S06_001", 56),
    Row("S08", "Crêpe de Chine", "Silk", 85.56149732620321, 0.5, "S08/001/head.dat", "6f5671d331594fdf110916a957b5de30244dc584", "1010_S08_001", 57),
    Row("S09", "Sanma Song Brocade", "Silk", 140.24632499006754, 0.3, "S09/001/head.dat", "b03131312e37507694c3e63e37fb8c52f68d5592", "136_S09_001", 58),
    Row("S10", "Song Brocade", "Silk", 217.86096256684493, 0.5, "S10/001/head.dat", "ae184774eb85cadc18db2d1e8ce39ae632abef22", "137_S10_001", 59),
    Row("S11", "Habotai-base Gambiered Guangdong Gauze", "Silk", 103.62263878354463, 0.4, "S11/001/head.dat", "c416908ebdb1252cfc87085542739f5bcb0f5059", "138_S11_001", 60),
    Row("S12", "Leno-base Gambiered Guangdong Gauze", "Silk", 130.13327554447937, 0.4, "S12/001/head.dat", "72f50a55f766f104a0fd814572a5cc6db1a12af7", "1014_S12_001", 61),
    Row("CU01", "Cupro Plain", "Cellulose-derived fibers", 68.11861162278325, 0.1, "CU01/001/head.dat", "d3c8489a96efce12e3b943a24a69608c6a3cfd89", "144_CU01_001", 62),
    Row("Drape 3", "Viscose White", "Cellulose-derived fibers", 225.43760587238847, 0.7, "Drape 3/001/head.dat", "9824cbb6e0df6a670a003f7fa46ac560f9ea04be", "189_Drape 3_001", 63),
    Row("EX-01", "Lyocell (Tencel™) Plain", "Cellulose-derived fibers", 122.7807486631016, 0.5, "EX-01/001/head.dat", "2297a961ad74b1e02e51e0beef6181b6118e8842", "1019_EX01_001", 64),
    Row("EX-02", "Lyocell Twill", "Cellulose-derived fibers", 165.4567125365695, 0.3, "EX-02/001/head.dat", "6ca38b583a680e5a492e330006f193839c2471c9", "204_EX-02_001", 65),
    Row("EX-05", "Bamboo Rayon Plain", "Cellulose-derived fibers", 251.12299465240642, 0.6, "EX-05/001/head.dat", "9e1b74645924f8b3539f57e7b85b0f762c7d4959", "206_EX-05_001", 67),
    Row("EX-06", "Modal Plain", "Cellulose-derived fibers", 153.8324420677362, 0.6, "EX-06/001/head.dat", "940c313688e0cf5d4747255d3dc195a2bbe9519a", "1016_EX06_001", 68),
    Row("R01", "Viscose Plain", "Cellulose-derived fibers", 63.567739372268576, 0.3, "R01/001/head.dat", "46a650a0b239d549095d7cc7616e29381cc69f06", "1005_R01_001", 69),
    Row("R02", "Viscose Twill", "Cellulose-derived fibers", 191.89511323003575, 0.3, "R02/001/head.dat", "aaf28b49ed69f517e8986c940314eff8780653b5", "141_R02_001", 70),
    Row("R03", "Viscose Satin", "Cellulose-derived fibers", 72.66948387329793, 0.3, "R03/001/head.dat", "c085dbc2cbb816f61c7e01110557f2e8786c02af", "1020_R03_001", 71),
    Row("Drape 6", "Duchess Satin", "Polyester", 186.8024704735074, 0.3, "Drape 6/001/head.dat", "adfa6cd06cd517a3e0a8568443d503ac4846d607", "192_Drape 6_001", 73),
    Row("Drape 12", "Plain Satin", "Polyester", 171.58385093167703, 0.7, "Drape 12/002/head.dat", "693f21d5319974fc8ee4111f0a6c5c724c3197fd", "198_Drape 12_002", 74),
    Row("E-F05", "Antistatic Polyester", "Polyester", 318.0748663101605, 0.6, "E-F05/001/head.dat", "221649f0d3f6e653c8e9a94456127f851ceee3f0", "1013_EF05_001", 75),
    Row("E-KN07", "Polyester Polar Fleece", "Polyester", 321.03, 1.2, "E-KN07/001/head.dat", "df9a5b546a70ee0a023c11d494580326cebff4f5", "1023_EKN07_001", 76),
    Row("E-P02", "Cationic Polyester", "Polyester", 121.89836385307184, 0.3, "E-P02/001/head.dat", "a1b1a7181bb35e3a06b47199d4286959ac6c6939", "171_E-P02_001", 77),
    Row("E-P03", "Polyester Peach Skin", "Polyester", 148.37288258027232, 0.4, "E-P03/001/head.dat", "fd7a6e799835852167573654e9a8755354dcbf3e", "172_E-P03_001", 78),
    Row("E-P04", "Polyester Suede", "Polyester", 185.45454545454547, 0.5, "E-P04/001/head.dat", "3dbf9d62a7570ae4bfba61f0f8cb7ff58ac5ca56", "173_E-P04_001", 79),
    Row("EX-08", "Coolmax Jersey", "Polyester", 128.73440285204993, 0.6, "EX-08/001/head.dat", "7a3fbfc11209129fa8bb979a895bb72b05418473", "1011_EX08_001", 80),
    Row("P01", "Polyester Taffeta", "Polyester", 83.61324809477372, 0.1, "P01/001/head.dat", "1e8da4e68ebe7e4bf1e8e85e971e2537cc8d48b2", "1002_P01_001", 82),
    Row("P02", "Polyester Chiffon", "Polyester", 97.73539928486294, 0.4, "P02/001/head.dat", "b6bcf2ad7a4285d15fe38a3d0cc7534e63ab8787", "146_P02_001", 83),
    Row("E-N01", "Nylon Taslon", "Nylon", 131.14458048903816, 0.4, "E-N01/001/head.dat", "c7e94fdead1567b9fb31147ed97dcb82e1d1304f", "174_E-N01_001", 84),
    Row("E-N02", "Nylon Oxford", "Nylon", 122.72907862895944, 0.2, "E-N02/002/head.dat", "a4fac63d7a49343c2e6b6b9be38e495e1559c745", "1021_EN02_002", 85),
    Row("N01", "Nylon Taffeta", "Nylon", 81.73511034059305, 0.2, "N01/001/head.dat", "272c407ff44220fe690c72d79d715073f106964a", "201_N01_001", 86),
    Row("N02", "Nylon Twill", "Nylon", 178.134142377289, 0.4, "N02/001/head.dat", "b9951c4542eb8b06872a6203925df0a5a4c115b1", "147_N02_001", 87),
    Row("A01", "Acrylic Wool-like", "Acrylic", 239.50089126559715, 0.5, "A01/001/head.dat", "ce7cfb7f18203b1b297fae15e5d33af6a0378f74", "148_A01_001", 88),
    Row("Drape 7", "Plain PolyCotton", "Blends", 105.39242243652257, 0.4, "Drape 7/001/head.dat", "7456e956f85acf86406e5d27b39b3579e2149826", "193_Drape 7_001", 89),
    Row("Drape 10", "Pale Gold Metallic Viscose", "Blends", 204.4748729531338, 0.7, "Drape 10/001/head.dat", "ea017aed07c810fe749621e17decd5b14e2c6d82", "196_Drape 10_001", 90),
    Row("E-C05", "Polyester/Cotton Plain", "Blends", 102.97251417632825, 0.4, "E-C05/001/head.dat", "ccd9ef1c191c89a2e06a13619c3f07bd449d6abf", "155_E-C05_001", 91),
    Row("E-C06", "Polyester/Cotton Twill", "Blends", 150.28713836818721, 0.4, "E-C06/001/head.dat", "1fd2c4924453cb5dbadcadda9fa6e2be9d608b87", "156_E-C06_001", 92),
    Row("E-L01", "Linen/Cotton Plain", "Blends", 143.38680926916223, 0.5, "E-L01/001/head.dat", "82f36702d9d031bc679da5c2d937cd7b0a7b562f", "161_E-L01_001", 93),
    Row("E-R02", "Viscose/Linen Plain", "Blends", 206.80926916221034, 0.6, "E-R02/001/head.dat", "fcae6b32537e27b59f57f7f499ae222029115720", "170_E-R02_001", 94),
    Row("E-W01", "Wool/Polyester Blend", "Blends", 176.54494889298226, 0.4, "E-W01/001/head.dat", "fd88b6084d89844c7e5d8bd6f0eec6fe10b82c69", "162_E-W01_001", 95),
    Row("E-W02", "Wool/Cashmere Blend", "Blends", 143.67753819482067, 0.4, "E-W02/001/head.dat", "c7ec2b3061201d5647af2eae4931accf30895e89", "163_E-W02_001", 96),
    Row("EX-04", "Modal/Cotton Jersey", "Blends", 230.76648841354725, 0.6, "EX-04/001/head.dat", "a3371c92647a01e1efd4c6fa08ccbdf946496590", "213_EX-04-1_001", 97),
    Row("EX-17", "Tencel™/Linen Blend", "Blends", 138.15147903348142, 0.4, "EX-17/001/head.dat", "e9dc96afd118354ba8485204d91636850d41532e", "186_EX-17_001", 98),
    Row("E-F01", "Coated Nylon", "Coated and functional fabrics", 41.89691913172247, 0.1, "E-F01/001/head.dat", "6ed7b4102ad8a8f045f56eae868a18e074f6db51", "1015_EF01_001", 99),
    Row("E-F02", "Waterproof Breathable", "Coated and functional fabrics", 126.12417379997834, 0.2, "E-F02/001/head.dat", "a5fb2beeced2556e2d4428457e1064f5892f5052", "1022_E-F02_001", 100),
    Row("EX-11", "Reflective Fabric", "Coated and functional fabrics", 129.88044930833965, 0.1, "EX-11/001/head.dat", "3e8c6cd8649c99399f83e462b023b68343a241e6", "1025_EX11_001", 101),
    Row("Paper 1", "Printing Paper (80 g)", "Paper", 83.152, 0.1, "Paper 1/001/head.dat", "de08889640e86289aaab2cd320d34687e8cb520f", "1026_Paper1_001", 102),
];

fn composition(id: &str) -> Vec<Component> {
    // Only same-ID, same-mass/thickness paper rows are joined to the release. Drape -> DP is the
    // explicit paper/table naming correspondence, checked by description and both measurements.
    // Nominal supplier/research composition is not an independently measured mixture law.
    let parts: &[(&str, f64)] = match id {
        "C01" | "C02" | "C03" | "C04" | "C05" | "C06" | "C07" | "C08" | "C09" | "C10" | "C11"
        | "C13" | "E-C01" | "E-C02" | "E-C03" | "E-C07" | "E-C08" | "E-C09" | "E-C10" | "KN01"
        | "KN02" | "KN03" | "E-KN01" | "E-KN03" | "E-KN04" | "E-KN05" | "Drape 1" | "Drape 2"
        | "Drape 9" => &[("cotton", 1.0)],
        "C12" => &[("cotton", 0.98), ("elastane", 0.02)],
        "E-C04" => &[("cotton", 0.97), ("elastane", 0.03)],
        "E-C05" | "Drape 7" | "Drape 11" => &[("polyester", 0.65), ("cotton", 0.35)],
        "E-C06" => &[("polyester", 0.80), ("cotton", 0.20)],
        "L01" | "L02" | "Drape 8" => &[("linen (flax bast)", 1.0)],
        "L03" => &[("ramie", 1.0)],
        "E-W03" | "E-W04" | "Drape 5" => &[("wool", 1.0)],
        "E-W01" => &[("wool", 0.70), ("polyester", 0.30)],
        "E-W02" => &[("wool", 0.90), ("cashmere", 0.10)],
        "S01" | "S02" | "S03" | "S04" | "S05" | "S06" | "E-S01" | "E-S02" | "E-S03" | "E-S04" => {
            &[("mulberry silk", 1.0)]
        }
        "CU01" => &[("cupro", 1.0)],
        "R01" | "R02" | "R03" => &[("viscose", 1.0)],
        "Drape 3" => &[("viscose", 0.95), ("elastane", 0.05)],
        "Drape 10" => &[("viscose", 0.70), ("polyester", 0.30)],
        "E-R02" => &[("viscose", 0.70), ("linen (flax bast)", 0.30)],
        "EX-01" | "EX-02" => &[("lyocell", 1.0)],
        "EX-04" => &[("modal", 0.50), ("cotton", 0.50)],
        "EX-05" => &[("viscose from bamboo (regenerated cellulose)", 1.0)],
        "EX-17" => &[("lyocell", 0.70), ("linen (flax bast)", 0.30)],
        "P01" | "P02" | "E-P02" | "E-P03" | "E-P04" | "EX-08" => &[("polyester", 1.0)],
        "N01" | "N02" | "E-N01" | "E-N02" => &[("nylon", 1.0)],
        "A01" => &[("acrylic", 1.0)],
        // Conflicting silk IDs, unquantified/coated blends, paper, and entries absent from the
        // concordant article table retain the release descriptor without invented fractions.
        _ => &[],
    };
    parts
        .iter()
        .map(|(fiber, fraction)| Component {
            fiber: (*fiber).into(),
            fraction: *fraction,
            source: "paper".into(),
        })
        .collect()
}

fn form(id: &str) -> Form {
    match id {
        "KN01" | "KN02" | "KN03" | "E-KN01" | "E-KN03" | "E-KN05" | "E-KN07" | "EX-04"
        | "EX-08" => Form::Knit,
        "E-F02" => Form::Laminate,
        // These names describe fibers, finishes or appearance without establishing a weave.
        // The instrument's universal 'woven' flag also occurs on explicit jersey/rib specimens.
        "A01" | "L02" | "Drape 3" | "Drape 5" | "Drape 8" | "Drape 10" | "Drape 11" | "W02"
        | "W03" | "W04" | "E-W01" | "E-W02" | "E-W03" | "E-W04" | "E-KN04" | "E-S01" | "E-S04"
        | "E-P02" | "E-P03" | "E-P04" | "E-F01" | "E-F05" | "EX-11" | "Paper 1" => Form::Sheet,
        _ => Form::Woven,
    }
}

fn discrepancy(id: &str) -> &'static str {
    match id {
        "S08" => {
            "Release S08 is Crepe de Chine, 0.5 mm, 85.56149732620321 g/m2; article Table 12 assigns a different brocade to S08. Do not join article S08 properties or composition."
        }
        "S09" => {
            "Release S09 is Sanma Song Brocade, 0.3 mm, 140.24632499006754 g/m2; article Table 12 assigns Song Brocade to S09. No cross-row reassignment."
        }
        "S10" => {
            "Release S10 is Song Brocade, 0.5 mm, 217.86096256684493 g/m2; article Table 12 assigns a different gauze to S10. No cross-row reassignment."
        }
        "S11" => {
            "Release S11 is Habotai-base Gambiered Guangdong Gauze, 0.4 mm, 103.62263878354463 g/m2; article Table 12 assigns leno-base gauze to S11. No cross-row reassignment."
        }
        "S12" => {
            "Release S12 is Leno-base Gambiered Guangdong Gauze, 0.4 mm, 130.13327554447937 g/m2; article Table 12 instead lists Heavy Silk Crepe. No cross-row reassignment."
        }
        "E-KN07" => {
            "Release CSV and instrument header agree on 321.03 g/m2; article Table 12 reports 231.71 g/m2. Retain the exact release observation, not the article value or an average."
        }
        "EX-06" => {
            "Release EX-06 is Modal Plain; the article lists that description/mass under EX-18. No cross-ID composition or mechanics join."
        }
        "L01" => {
            "Warp s1 raw timestamps imply median positive twist rate 0.499854 degrees/s and compression rate 0.75 mm/s, versus article 0.1 degrees/s and 1 mm/s. Peak compression 16.14125 mm includes 1.14125 mm pretension displacement. Raw axes must not be converted using nominal geometry alone."
        }
        _ => {
            "Global protocol is reported by the article; per-run timestamps, pretension, tare, sequence and raw curve identity require separate qualification before any calibration."
        }
    }
}

pub(super) fn records() -> Vec<Material> {
    ROWS.iter().map(|&Row(id, name, category, mass, thickness, header, blob, specimen, line)| {
        let directory = header.rsplit_once('/').unwrap().0;
        let components = composition(id);
        let quantity = |value, unit: &str, method: &str| Quantity {
            value, unit: unit.into(), evidence: Evidence::Measured,
            source: "release".into(), method: method.into(), bounds: None,
            conditions: BTreeMap::from([
                ("specimen".into(), specimen.into()),
                ("source_fabric_id".into(), id.into()),
                ("source_csv_line".into(), line.to_string()),
                ("release_revision".into(), REVISION.into()),
                ("conditioning".into(), CONDITIONING.into()),
                ("replication".into(), "Number of density/thickness repeats and uncertainty not reported; CSV digits are not an accuracy claim".into()),
                ("metadata_concordance".into(), "Release CSV agrees with the selected exact instrument metadata header within its printed numerical precision".into()),
            ]),
        };
        let mut reported_thickness = quantity(thickness, "mm",
            "fabric_catalog.csv thickness_mm, checked against instrument Thickness(mm); paper section 3.1 fabric thickness tester");
        reported_thickness.conditions.extend([
            ("measurement_pressure".into(), "Presser pressure, foot area and dwell time not reported".into()),
            ("applicability".into(), "Reported test thickness only; not unloaded physical thickness, contact separation or a compression law".into()),
        ]);
        Material {
            id: format!("fabric101-2026/{}", id.to_ascii_lowercase().replace(' ', "-")),
            name: format!("Fabric-101 {id}: {name}"),
            form: form(id),
            composition_complete: !components.is_empty(),
            composition: components,
            structure: BTreeMap::from([
                ("specimen".into(), specimen.into()),
                ("source_fabric_id".into(), id.into()),
                ("source_category".into(), category.into()),
                ("source_descriptor".into(), name.into()),
                ("source_revision".into(), REVISION.into()),
                ("source_csv_line".into(), line.to_string()),
                ("raw_experiment_directory".into(), format!("{REPOSITORY}/tree/{REVISION}/data/{}", directory.replace(' ', "%20"))),
                ("raw_metadata_path".into(), format!("data/{header}")),
                ("raw_metadata_git_blob_sha1".into(), blob.into()),
                ("construction".into(), match form(id) {
                    Form::Woven => "woven",
                    Form::Knit => "knit",
                    Form::Laminate => "laminate",
                    _ => "unspecified sheet construction",
                }.into()),
                ("composition_basis".into(), "Only concordant article Tables 11-13 nominal composition; no independent chemical verification or fiber-to-fabric mixture law. Empty composition means unqualified fractions, not absent fibers.".into()),
                ("source_discrepancy".into(), discrepancy(id).into()),
                ("raw_test_geometry".into(), "Article section 3.1: 260 x 110 mm rectangle on radius 40 mm cylindrical holders, 40/20 mm clamps, nominal free height 50 mm; actual pretension displacement must be preserved.".into()),
                ("raw_test_sequence".into(), "Five twist loading/unloading cycles to 5 degrees, five axial compression cycles with nominal 15 mm travel, then axial stretch until upper-clamp slip; force N, displacement mm, torque N*m, rotation degrees and time s.".into()),
                ("mechanical_scope".into(), "Cylinder force/torque and post-buckling/slip curves remain raw fixture responses. No sheet modulus, shear law, bending rigidity, Poisson coupling, contact coefficient or independent validation is inferred.".into()),
            ]),
            properties: BTreeMap::from([
                ("areal_density".into(), quantity(mass, "g/m2",
                    "fabric_catalog.csv areal_density_g_m2, checked against instrument Weight(gsm); paper section 3.1 balance resolution 0.001 g")),
                ("reported_thickness".into(), reported_thickness),
            ]),
            membrane_response: None, membrane_relaxation: None,
            thermal_response: None, optical_faces: None,
            sources: BTreeMap::from([
                ("release".into(), Source {
                    reference: format!("{REPOSITORY}/blob/{REVISION}/fabric_catalog.csv"),
                    accessed_on: "2026-10-27".into(),
                    note: format!("Authors' Fabric-101 numerical catalog, line {line}, exact ID {id}; Git commit {REVISION}; file SHA-256 {CATALOG_SHA256}. Original g/m2 and mm retained. Only selected factual observations are transcribed; no upstream implementation or raw curve archive is included."),
                }),
                ("specimen".into(), Source {
                    reference: format!("{REPOSITORY}/blob/{REVISION}/data/{}", header.replace(' ', "%20")),
                    accessed_on: "2026-10-27".into(),
                    note: format!("Exact instrument Material ID {specimen}; metadata Git blob SHA-1 {blob}. Density and thickness checked against release CSV. Personal/operator/manufacturer fields are redacted by upstream. Instrument construction field is not used to misclassify knits."),
                }),
                ("mapping".into(), Source {
                    reference: format!("{REPOSITORY}/blob/{REVISION}/data_folder_mapping.csv"),
                    accessed_on: "2026-10-27".into(),
                    note: format!("Authors' original-folder to release-ID mapping, matched_catalog_id row for {id}; file SHA-256 {MAPPING_SHA256}. Five named demonstration folders are not catalog-ID matches."),
                }),
                ("paper".into(), Source {
                    reference: ARTICLE.into(),
                    accessed_on: "2026-10-27".into(),
                    note: "Gong, Mao, Yang, Lu, Wang and Komura, Learnable Persistent Wrinkle Formation in Cloth Simulation, arXiv:2609.13707v1, 12 September 2026, SIGGRAPH Asia 2026. Section 3.1 supplies reported apparatus/conditioning; Tables 11-13 supply nominal compositions only for concordant identities. Release/header observations take precedence over conflicting article rows. README advertises MIT, but this pinned tree has no LICENSE file; no license grant is asserted for upstream code or raw datasets.".into(),
                }),
            ]),
        }
    }).collect()
}
